#!/usr/bin/env node
// lycodl-driver-version: 1
// 下载完成后自动发布 driver（可替换）。
// stdin: 一个 JSON 对象；stdout: 每行一条 {"dest":...,"status":...,"detail":...,"url":...}
// status: ok | failed | unavailable | dry-run
'use strict';

import { spawnSync } from 'node:child_process';

const PLATFORMS = ['bilibili', 'douyin', 'xhs', 'kuaishou', 'youtube'];
const DEFAULT_SHARE = 'https://pan.quark.cn/s/cb7e803942d9';
const DEFAULT_PASSCODE = 'xdDX';
const DEFAULT_QUARK_DIR = '/06_待整理/lycodl-短剧集';
const DEFAULT_QUAKE = '/home/lilyco/.config/kuake/run.sh';

function emit(o) { process.stdout.write(JSON.stringify(o) + '\n'); }

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => { input += c; });
process.stdin.on('end', () => {
  main(input).catch((e) => {
    emit({ dest: 'driver', status: 'failed', detail: String((e && e.stack) || e).slice(0, 800) });
    process.exitCode = 1;
  });
});

function truncateBytes(s, n) {
  let out = '', used = 0;
  for (const ch of String(s || '')) {
    const len = Buffer.byteLength(ch);
    if (used + len > n) break;
    out += ch; used += len;
  }
  return out;
}

function findLastJson(text) {
  const re = /\{\s*["']success["']\s*:/g;
  const hits = [];
  let m;
  while ((m = re.exec(text)) !== null) hits.push(m.index);
  for (let i = hits.length - 1; i >= 0; i -= 1) {
    const tail = text.slice(hits[i]).trim();
    try { return JSON.parse(tail); } catch (e) { /* try older object */ }
  }
  return null;
}

async function probeCdp(cdpUrl) {
  try {
    const url = cdpUrl.replace(/\/+$/, '') + '/json/list';
    const r = await fetch(url, { signal: AbortSignal.timeout(3000) });
    if (!r.ok) return null;
    const targets = await r.json();
    return { targets: Array.isArray(targets) ? targets : [], url: cdpUrl };
  } catch (e) { return null; }
}

class Cdp {
  constructor(ws) {
    this.ws = ws; this.id = 0; this.waiting = new Map();
    ws.addEventListener('message', (ev) => {
      let m; try { m = JSON.parse(String(ev.data)); } catch (e) { return; }
      if (!m || !m.id || !this.waiting.has(m.id)) return;
      const p = this.waiting.get(m.id); this.waiting.delete(m.id);
      if (m.error) p.reject(new Error(JSON.stringify(m.error))); else p.resolve(m.result);
    });
  }
  static connect(wsUrl) {
    return new Promise((resolve, reject) => {
      let ws;
      try { ws = new WebSocket(wsUrl); } catch (e) { reject(e); return; }
      const timer = setTimeout(() => reject(new Error('CDP 连接超时')), 6000);
      ws.addEventListener('open', () => { clearTimeout(timer); resolve(new Cdp(ws)); });
      ws.addEventListener('error', () => { clearTimeout(timer); reject(new Error('CDP 连接失败')); });
    });
  }
  send(method, params) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.waiting.delete(id)) reject(new Error('CDP 超时: ' + method));
      }, 20000);
      this.waiting.set(id, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      try { this.ws.send(JSON.stringify({ id, method, params: params || {} })); }
      catch (e) { this.waiting.delete(id); clearTimeout(timer); reject(e); }
    });
  }
  async evaluate(expression) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r && r.exceptionDetails) throw new Error(r.exceptionDetails.text || 'Runtime.evaluate 异常');
    return r && r.result ? r.result.value : undefined;
  }
  close() { try { this.ws.close(); } catch (e) {} }
}

async function withCdp(target, fn) {
  const c = await Cdp.connect(target.webSocketDebuggerUrl);
  try { return await fn(c); } finally { c.close(); }
}

function shellResult(out, dest) {
  const text = String(out.stdout || '');
  const j = findLastJson(text);
  if (!j) {
    return { dest, status: 'failed', detail: ('无有效 JSON 输出: ' + String(out.stderr || text).trim()).slice(0, 400) };
  }
  const ok = j.success === true || j.status === 'ok';
  return {
    dest,
    status: ok ? 'ok' : 'failed',
    detail: (j.message || j.detail || JSON.stringify(j)).toString().slice(0, 400),
    url: j.url || j.share_url || null,
  };
}

async function doQuark(req) {
  const remoteDir = req.quarkDir || DEFAULT_QUARK_DIR;
  const name = truncateBytes(req.fileName || 'video.mp4', 120);
  const remote = remoteDir.replace(/\/+$/, '') + '/' + name;
  const cmd = req.kuakeCommand || DEFAULT_QUAKE;
  const timeout = Math.max(60, Number(req.timeoutSecs || 7200)) * 1000;
  const out = spawnSync(cmd, ['upload', req.file, remote], {
    encoding: 'utf8', timeout, maxBuffer: 32 * 1024 * 1024,
  });
  if (out.error) return { dest: 'quark', status: 'failed', detail: String(out.error).slice(0, 400) };
  const r = shellResult(out, 'quark');
  if (r.status === 'ok') r.detail = '已上传夸克: ' + remote;
  return r;
}

async function doX(req, cdp, share, passcode) {
  if (!cdp) return { dest: 'x', status: 'unavailable', detail: 'CDP 调试浏览器未启动，X 未发布' };
  const target = cdp.targets.find((t) => t.type === 'page' && /(^|\/\/)(x|twitter)\.com\//.test(String(t.url || '')));
  if (!target || !target.webSocketDebuggerUrl) {
    return { dest: 'x', status: 'unavailable', detail: 'CDP 中没有已登录的 x.com 页面，X 未发布' };
  }
  const text = truncateBytes('古裝短劇合集｜' + (req.title || req.fileName || ''), 140) + '\n' + share + '  提取码 ' + passcode;
  try {
    return await withCdp(target, async (c) => {
      const focused = await c.evaluate('(()=>{const e=document.querySelector(\'[data-testid="tweetTextbox_0"]\');if(!e)return false;e.focus();return true;})()');
      if (!focused) return { dest: 'x', status: 'failed', detail: 'X 页面未找到推文输入框（可能未登录）' };
      await c.send('Input.insertText', { text });
      await new Promise((r) => setTimeout(r, 500));
      const clicked = await c.evaluate('(()=>{const b=document.querySelector(\'[data-testid="tweetButtonInline"]\');if(!b||b.disabled)return false;b.click();return true;})()');
      if (!clicked) return { dest: 'x', status: 'failed', detail: 'X 发布按钮不可用（可能超出字数或未填写）' };
      await new Promise((r) => setTimeout(r, 1200));
      return { dest: 'x', status: 'ok', detail: '已提交 X 推文（网盘链接 + 提取码）', url: await c.evaluate('location.href') };
    });
  } catch (e) { return { dest: 'x', status: 'failed', detail: String(e.message || e).slice(0, 400) }; }
}

async function doPlatform(req, dest, cdp, platformCommand, share, passcode) {
  const payload = Object.assign({}, req, { dest, shareUrl: share, sharePasscode: passcode });
  const timeout = Math.max(60, Number(req.timeoutSecs || 7200)) * 1000;
  if (platformCommand) {
    const out = spawnSync('sh', ['-c', platformCommand], {
      input: JSON.stringify(payload), encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024,
    });
    if (out.error) return { dest, status: 'failed', detail: String(out.error).slice(0, 400) };
    const lines = String(out.stdout || '').trim().split('\n').filter(Boolean);
    const line = lines[lines.length - 1];
    if (!line) return { dest, status: 'failed', detail: '平台提交器无输出: ' + String(out.stderr || '').slice(0, 300) };
    try {
      const j = JSON.parse(line);
      return { dest, status: j.status === 'ok' ? 'ok' : 'failed', detail: String(j.detail || line).slice(0, 400), url: j.url || null };
    } catch (e) { return { dest, status: 'failed', detail: ('平台提交器输出解析失败: ' + line).slice(0, 400) }; }
  }
  if (!cdp) return { dest, status: 'unavailable', detail: 'CDP 未启动且未配置平台提交器，' + dest + ' 未发布' };
  const target = cdp.targets.find((t) => t.type === 'page' && /multipost\.app/.test(String(t.url || '')));
  if (!target || !target.webSocketDebuggerUrl) {
    return { dest, status: 'unavailable', detail: 'CDP 中没有 MultiPost 发布页，且未配置平台提交器，' + dest + ' 未发布' };
  }
  try {
    return await withCdp(target, async (c) => {
      const hasHook = await c.evaluate("typeof window.__lycodlPublishPlatform === 'function'");
      if (!hasHook) {
        return { dest, status: 'unavailable', detail: 'MultiPost 页面未暴露 __lycodlPublishPlatform；可配置「平台提交命令」实现 ' + dest + ' 自动提交' };
      }
      const expr = 'window.__lycodlPublishPlatform(' + JSON.stringify({ dest, file: req.file, title: req.title, fileName: req.fileName, shareUrl: share, sharePasscode: passcode }) + ')';
      const v = await c.evaluate(expr);
      const j = (v && typeof v === 'object') ? v : { status: 'ok', detail: String(v || '') };
      return { dest, status: j.status === 'ok' ? 'ok' : (j.status || 'failed'), detail: String(j.detail || '').slice(0, 400), url: j.url || null };
    });
  } catch (e) { return { dest, status: 'failed', detail: String(e.message || e).slice(0, 400) }; }
}

async function main(raw) {
  let req;
  try { req = JSON.parse(raw); } catch (e) { throw new Error('driver 输入不是合法 JSON: ' + e.message); }
  const pending = Array.isArray(req.pending) ? req.pending : [];
  const dry = !!req.dryRun;
  const share = req.shareUrl || DEFAULT_SHARE;
  const passcode = req.sharePasscode || DEFAULT_PASSCODE;
  const cdpUrl = req.cdpUrl || 'http://127.0.0.1:9333';
  const platformCommand = req.platformCommand || '';
  let cdp = null;
  if (!dry && pending.some((d) => d === 'x' || PLATFORMS.indexOf(d) >= 0)) cdp = await probeCdp(cdpUrl);
  for (const dest of pending) {
    try {
      if (dry) { emit({ dest, status: 'dry-run', detail: '演练：' + (req.fileName || req.file) + ' 将发布到 ' + dest }); continue; }
      let r;
      if (dest === 'quark') r = await doQuark(req);
      else if (dest === 'x') r = await doX(req, cdp, share, passcode);
      else if (PLATFORMS.indexOf(dest) >= 0) r = await doPlatform(req, dest, cdp, platformCommand, share, passcode);
      else r = { dest, status: 'failed', detail: '未知目的地' };
      emit(r);
    } catch (e) { emit({ dest, status: 'failed', detail: String(e.message || e).slice(0, 400) }); }
  }
}
