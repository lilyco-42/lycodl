#!/usr/bin/env node
// lycodl-adapter-runner-version: 1
//
// Generic publish runner: executes a declarative adapter (adapters/<id>.json)
// over CDP. Adding a platform means adding an adapter file, not editing this.
//
// CLI: stdin {adapter, file, fileName, title, shareUrl, sharePasscode, visibility, cdpUrl}
//      stdout one JSON line: {status, detail, url}
'use strict';

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { connectBrowser, newPage, closePage } from './cdp-util.mjs';

const ADAPTER_DIR = fileURLToPath(new URL('./adapters/', import.meta.url));

function adapterPath(id) {
  return fileURLToPath(new URL(`./adapters/${id}.json`, import.meta.url));
}

export function loadAdapter(id) {
  const p = adapterPath(String(id || ''));
  if (!existsSync(p)) return null;
  return JSON.parse(readFileSync(p, 'utf8'));
}

export function listAdapters() {
  if (!existsSync(ADAPTER_DIR)) return [];
  return readdirSync(ADAPTER_DIR)
    .filter((f) => f.endsWith('.json') && f !== 'schema.json')
    .map((f) => f.slice(0, -5));
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function sliceBytes(s, n) {
  if (!n) return String(s || '');
  const b = Buffer.from(String(s || ''), 'utf8');
  return b.length <= n ? String(s || '') : b.subarray(0, n).toString('utf8');
}

function expand(value, vars) {
  return String(value == null ? '' : value).replace(/\$([a-zA-Z][a-zA-Z0-9]*)/g, (m, k) =>
    Object.prototype.hasOwnProperty.call(vars, k) ? String(vars[k]) : m,
  );
}

function textsExpr(texts) {
  return '[' + texts.map((t) => JSON.stringify(t)).join(',') + ']';
}

async function clickByText(page, texts, opts = {}) {
  const selector = opts.selector || 'button,a,div,span';
  const expr = `(function(){var want=${textsExpr(texts)};var list=document.querySelectorAll(${JSON.stringify(selector)});var cand=[];for(var i=0;i<list.length;i++){var e=list[i];var t=(e.innerText||e.textContent||"").trim();if(want.indexOf(t)<0)continue;if(e.disabled)continue;if(!e.getClientRects().length)continue;cand.push(e);}if(!cand.length)return null;var b=cand[cand.length-1];try{b.scrollIntoView({block:"center"});}catch(x){}b.click();return (b.innerText||"").trim().slice(0,20);})()`;
  return page.evaluate(expr);
}

async function clickDeep(page, texts, preferTag) {
  const expr = `(function(){var want=${textsExpr(texts)};function all(root,out,d){if(d>8)return;var list;try{list=root.querySelectorAll("*");}catch(e){return;}for(var i=0;i<list.length;i++){var e=list[i];out.push(e);if(e.shadowRoot)all(e.shadowRoot,out,d+1);else if(e._sr)all(e._sr,out,d+1);}}var nodes=[];all(document,nodes,0);var cand=[];for(var i=0;i<nodes.length;i++){var e=nodes[i];var t=(e.innerText||e.textContent||"").trim();if(want.indexOf(t)<0)continue;if(e.disabled)continue;if(!e.getClientRects().length)continue;cand.push(e);}if(!cand.length)return null;var pick=${JSON.stringify(preferTag || '')}?cand.filter(function(e){return e.tagName===${JSON.stringify(preferTag || '')};}):[];var b=pick.length?pick[0]:cand[cand.length-1];try{b.scrollIntoView({block:"center"});}catch(x){}b.click();return {t:(b.innerText||"").trim(),tag:b.tagName,n:cand.length};})()`;
  return page.evaluate(expr);
}

async function fillField(page, selector, value) {
  const focused = await page.evaluate(`(function(){var e=document.querySelector(${JSON.stringify(selector)});if(!e)return false;e.focus();try{if(e.select)e.select();}catch(x){}return true;})()`);
  if (!focused) return false;
  const text = String(value || '');
  if (text) await page.send('Input.insertText', { text });
  await sleep(300);
  return true;
}

async function waitTest(page, test) {
  const timeoutMs = Number(test.timeoutMs || 180000);
  const intervalMs = Number(test.intervalMs || 1500);
  const deadline = Date.now() + timeoutMs;
  const expr = test.expr
    || `(function(){var t=document.body.innerText||"";var want=${textsExpr(test.texts || [])};for(var i=0;i<want.length;i++){if(t.indexOf(want[i])>=0)return "ok";}${test.urlIncludes ? `if(location.href.indexOf(${JSON.stringify(test.urlIncludes)})>=0)return "ok";` : ''}return false;})()`;
  while (Date.now() < deadline) {
    const ok = await page.evaluate(expr).catch(() => false);
    if (ok) return true;
    await sleep(intervalMs);
  }
  return false;
}

async function runStep(page, step, vars) {
  const timeoutMs = Number(step.timeoutMs || 60000);
  switch (step.action) {
    case 'goto':
      await page.goto(expand(step.url, vars), timeoutMs);
      return true;
    case 'waitFor':
      return waitTest(page, { expr: step.expr, timeoutMs: step.timeoutMs, intervalMs: step.intervalMs });
    case 'waitMs':
      await sleep(Number(step.ms || 0));
      return true;
    case 'setFile': {
      const selector = step.selector || 'input[type=file]';
      return page.setFile(selector, vars.file);
    }
    case 'fill':
      return fillField(page, step.selector, sliceBytes(expand(step.value, vars), step.maxBytes));
    case 'clickText':
      return clickByText(page, (step.texts || []).map((t) => expand(t, vars)), { selector: step.selector });
    case 'clickDeep':
      return clickDeep(page, (step.texts || []).map((t) => expand(t, vars)), step.preferTag);
    case 'eval':
      return page.evaluate(expand(step.expr, vars));
    case 'press':
      await page.send('Input.dispatchKeyEvent', { type: 'keyDown', key: step.key || 'Enter' });
      await page.send('Input.dispatchKeyEvent', { type: 'keyUp', key: step.key || 'Enter' });
      return true;
    default:
      throw new Error('unknown adapter action: ' + step.action);
  }
}

export async function runAdapter(adapter, req) {
  const vars = {
    file: req.file,
    fileName: req.fileName || (req.file ? String(req.file).split(/[\\/]/).pop() : ''),
    title: req.title || '',
    desc: [req.title || '', req.shareUrl ? '网盘：' + req.shareUrl : '', req.sharePasscode ? '提取码：' + req.sharePasscode : '']
      .filter(Boolean).join(' '),
    shareUrl: req.shareUrl || '',
    sharePasscode: req.sharePasscode || '',
    visibility: req.visibility || 'public',
  };

  let browser = null;
  let page = null;
  let targetId = null;
  try {
    browser = await connectBrowser(req.cdpUrl);
    ({ page, targetId } = await newPage(browser, 'about:blank'));
    for (const step of adapter.steps || []) {
      try {
        const r = await runStep(page, step, vars);
        if (step.action === 'waitFor' && !r && !step.optional) {
          return { status: 'failed', detail: 'adapter 等待超时: ' + step.expr };
        }
      } catch (e) {
        if (!step.optional) throw e;
      }
    }
    const ok = await waitTest(page, adapter.result.success || {});
    const url = adapter.result.urlExpr
      ? await page.evaluate(adapter.result.urlExpr).catch(() => null)
      : null;
    if (!ok) {
      const tail = String(await page.evaluate('(document.body.innerText||"").slice(-220)').catch(() => '')).replace(/\n/g, ' | ');
      return { status: 'failed', detail: '未捕获发布成功 | ' + tail, url };
    }
    return { status: 'ok', detail: adapter.result.detail || (adapter.name + ' 已发布'), url };
  } finally {
    if (browser && targetId) await closePage(browser, targetId);
    if (browser) browser.close();
  }
}

if (process.argv[1] && process.argv[1].endsWith('adapter-runner.mjs')) {
  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (c) => { input += c; });
  process.stdin.on('end', async () => {
    let req = {};
    try { req = JSON.parse(input || '{}'); } catch { req = {}; }
    const adapter = loadAdapter(req.adapter);
    if (!adapter) {
      process.stdout.write(JSON.stringify({ status: 'unavailable', detail: '未知 adapter: ' + req.adapter }) + '\n');
      return;
    }
    try {
      const r = await runAdapter(adapter, req);
      process.stdout.write(JSON.stringify(r) + '\n');
    } catch (e) {
      process.stdout.write(JSON.stringify({ status: 'failed', detail: String((e && e.message) || e).slice(0, 500) }) + '\n');
    }
  });
}