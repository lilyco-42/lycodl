// lycodl CDP 辅助库（供 publish-driver 的平台提交器使用）
'use strict';
import http from 'node:http';

export function httpJson(url, timeout = 4000) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, (res) => {
      let d = '';
      res.on('data', (c) => { d += c; });
      res.on('end', () => { try { resolve(JSON.parse(d)); } catch (e) { reject(new Error('非 JSON: ' + d.slice(0, 120))); } });
    });
    req.on('error', reject);
    req.setTimeout(timeout, () => { req.destroy(new Error('HTTP 超时')); });
  });
}

export class Conn {
  constructor(ws, sessionId) { this.ws = ws; this.sessionId = sessionId || null; this.id = 0; this.waiting = new Map();
    ws.addEventListener('message', (ev) => {
      let m; try { m = JSON.parse(String(ev.data)); } catch (e) { return; }
      if (!m || !m.id || !this.waiting.has(m.id)) return;
      const p = this.waiting.get(m.id); this.waiting.delete(m.id);
      if (m.error) p.reject(new Error(JSON.stringify(m.error))); else p.resolve(m.result);
    });
  }
  static connect(wsUrl, sessionId) {
    return new Promise((resolve, reject) => {
      let ws;
      try { ws = new WebSocket(wsUrl); } catch (e) { reject(e); return; }
      const timer = setTimeout(() => reject(new Error('CDP 连接超时')), 8000);
      ws.addEventListener('open', () => { clearTimeout(timer); resolve(new Conn(ws, sessionId)); });
      ws.addEventListener('error', () => { clearTimeout(timer); reject(new Error('CDP 连接失败')); });
    });
  }
  send(method, params) {
    const id = ++this.id;
    const msg = { id, method, params: params || {} };
    if (this.sessionId) msg.sessionId = this.sessionId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { if (this.waiting.delete(id)) reject(new Error('CDP 超时: ' + method)); }, 30000);
      this.waiting.set(id, { resolve: (v) => { clearTimeout(timer); resolve(v); }, reject: (e) => { clearTimeout(timer); reject(e); } });
      try { this.ws.send(JSON.stringify(msg)); } catch (e) { this.waiting.delete(id); clearTimeout(timer); reject(e); }
    });
  }
  async evaluate(expression, timeoutMs) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r && r.exceptionDetails) throw new Error('evaluate 异常: ' + (r.exceptionDetails.text || '') + ' ' + JSON.stringify(r.exceptionDetails.exception && r.exceptionDetails.exception.description || '').slice(0, 300));
    return r && r.result ? r.result.value : undefined;
  }
  async waitFor(expression, timeoutMs = 30000, intervalMs = 500) {
    const t0 = Date.now();
    let last;
    let body;
    while (Date.now() - t0 < timeoutMs) {
      try {
        last = await this.evaluate(expression);
        if (last) return last;
        if (this.sessionId && Date.now() - t0 > 10000) {
          body = await this.evaluate('(document.body&&document.body.innerText||"").slice(0,900)').catch(() => null);
          const fail = String(body || '');
          if (/上传失败|上传异常|发布失败|网络异常|请重新上传|文件过大|格式不支持|登录已失效|身份验证|会话已过期/.test(fail)) {
            throw new Error('页面报告失败：' + fail.split(String.fromCharCode(10)).filter(Boolean).slice(-6).join(' | ').slice(0, 320));
          }
        }
      } catch (e) {
        if (String(e.message || e).indexOf('页面报告失败') >= 0) throw e;
        last = 'err:' + e.message;
      }
      await new Promise((r) => setTimeout(r, intervalMs));
    }
    const tail = String(body || '').split(String.fromCharCode(10)).filter(Boolean).slice(-5).join(' | ').slice(0, 260);
    throw new Error('等待超时(' + timeoutMs + 'ms): ' + expression.slice(0, 120) + ' 页面=' + (tail || '(空)') + ' 最后值=' + JSON.stringify(last).slice(0, 160));
  }
  async goto(url, waitMs = 3000) {
    await this.send('Page.navigate', { url });
    await new Promise((r) => setTimeout(r, waitMs));
  }
  async setFile(selector, filePath) {
    const doc = await this.send('DOM.getDocument', { depth: -1, pierce: true });
    const q = await this.send('DOM.querySelector', { nodeId: doc.root.nodeId, selector });
    if (!q || !q.nodeId) throw new Error('找不到文件输入框: ' + selector);
    await this.send('DOM.setFileInputFiles', { files: [filePath], nodeId: q.nodeId });
    return true;
  }
  async clickSel(selector) {
    return this.evaluate('(()=>{const e=document.querySelector(' + JSON.stringify(selector) + ');if(!e)return "missing";e.scrollIntoView({block:"center"});e.click();return "clicked";})()');
  }
  async typeInto(selector, text) {
    const ok = await this.evaluate('(()=>{const e=document.querySelector(' + JSON.stringify(selector) + ');if(!e)return false;e.focus();if(e.isContentEditable){e.textContent="";}else{e.value="";}return true;})()');
    if (!ok) throw new Error('找不到输入框: ' + selector);
    await this.send('Input.insertText', { text });
    // 触发 input/change，便于框架感知
    await this.evaluate('(()=>{const e=document.querySelector(' + JSON.stringify(selector) + ');if(e){e.dispatchEvent(new Event("input",{bubbles:true}));e.dispatchEvent(new Event("change",{bubbles:true}));}return true;})()');
    return true;
  }
  /** 在 closed shadow root 内查找并点击（小红书等使用 closed shadow DOM 的站点） */
  async clickInShadowChain(selectorPath, textHint) {
    const expr = '(()=>{const path=' + JSON.stringify(selectorPath) + ';const hint=' + JSON.stringify(textHint || '') + ';'
      + 'function walk(root,depth){ if(depth>8) return null; const list=root.querySelectorAll? Array.from(root.querySelectorAll("*")):[];'
      + ' return list; }'
      + 'const all=[]; (function rec(root,depth){ if(depth>6)return; for(const e of Array.from(root.querySelectorAll?root.querySelectorAll("*"):[])){ all.push(e); if(e.shadowRoot) rec(e.shadowRoot,depth+1); else if(e._sr) rec(e._sr,depth+1); } })(document,0);'
      + 'for(const e of all){ if(!hint||(e.innerText||"").trim()===hint){ if(path && !e.matches(path)) continue; return "found"; } } return "notfound";})()';
    return this.evaluate(expr);
  }
  close() { try { this.ws.close(); } catch (e) {} }
}

/** 打开一个新标签页并 attach（flatten），返回 page 会话连接的目标信息 */
export async function newPage(browser, url) {
  const { targetId } = await browser.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browser.send('Target.attachToTarget', { targetId, flatten: true });
  const page = new Conn(browser.ws, sessionId);
  await page.send('Runtime.enable');
  await page.send('Page.enable');
  await page.send('DOM.enable');
  await page.send('Network.enable');
  if (url) await page.goto(url, 2500);
  return { page, targetId };
}

export async function closePage(browser, targetId) {
  try { await browser.send('Target.closeTarget', { targetId }); } catch (e) { /* ignore */ }
}

export function truncateBytes(s, n) {
  let out = '', used = 0;
  for (const ch of String(s || '')) { const len = Buffer.byteLength(ch); if (used + len > n) break; out += ch; used += len; }
  return out;
}
