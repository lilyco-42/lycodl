#!/usr/bin/env node
// CI smoke: load the MV3 extension into headless Chrome and run one adapter
// through chrome.debugger. No accounts, no publishing, no Windows needed.
//
// Usage: node scripts/extension-smoke.mjs [--chrome /path/to/chrome]
'use strict';

import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const EXT = join(ROOT, 'extension');
const argChrome = process.argv.indexOf('--chrome');
const CHROME = argChrome > 0 ? process.argv[argChrome + 1] : (process.env.CHROME_BIN || '/usr/bin/google-chrome-stable');
const PORT = 9331;

if (typeof WebSocket === 'undefined') {
  console.error('extension smoke needs Node 22+ (global WebSocket)');
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitCdp() {
  for (let i = 0; i < 120; i += 1) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      if (r.ok) return await r.json();
    } catch {}
    await sleep(250);
  }
  throw new Error('chrome did not expose CDP');
}

class Conn {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.ready = new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      const p = m.id && this.pending.get(m.id);
      if (!p) return;
      this.pending.delete(m.id);
      m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result);
    };
  }
  async send(method, params = {}) {
    await this.ready;
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.has(id)) { this.pending.delete(id); reject(new Error('timeout ' + method)); }
      }, 30000);
    });
  }
}

async function connect(wsUrl) {
  return new Conn(new WebSocket(wsUrl));
}

async function main() {
  const profile = await mkdtemp(join(tmpdir(), 'lycodl-ext-smoke-'));
  const chrome = spawn(CHROME, [
    '--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage',
    `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', 'about:blank',
  ], { stdio: 'ignore' });

  try {
    const ver = await waitCdp();
    const browser = await connect(ver.webSocketDebuggerUrl);
    const loaded = await browser.send('Extensions.loadUnpacked', { path: EXT });
    if (!loaded || !loaded.id) throw new Error('extension failed to load');
    console.log('extension id', loaded.id);

    await sleep(2000);
    let sw = null;
    for (let i = 0; i < 20 && !sw; i += 1) {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
      sw = list.find((t) => t.type === 'service_worker' && t.url.includes(loaded.id));
      if (!sw) await sleep(500);
    }
    if (!sw) throw new Error('extension service worker not found');

    const page = await connect(sw.webSocketDebuggerUrl);
    const has = await page.send('Runtime.evaluate', { expression: 'typeof self.lycodlRunAdapter', returnByValue: true });
    if (has.result.value !== 'function') throw new Error('executor not exposed: ' + has.result.value);

    const adapter = {
      id: 'smoke',
      name: 'smoke',
      origin: 'https://example.com',
      steps: [
        { action: 'goto', url: 'https://example.com' },
        { action: 'waitFor', expr: "document.title.indexOf('Example')>=0", timeoutMs: 30000 },
      ],
      result: { success: { expr: "document.title.indexOf('Example')>=0" }, detail: 'extension executor ok' },
    };
    const r = await page.send('Runtime.evaluate', {
      expression: `self.lycodlRunAdapter(${JSON.stringify(adapter)}, {file:'',fileName:''})`,
      returnByValue: true,
      awaitPromise: true,
    });
    const result = r.result && r.result.value;
    console.log('executor result', JSON.stringify(result));
    if (!result || result.status !== 'ok') throw new Error('adapter smoke failed: ' + JSON.stringify(result));

    // Web page -> content bridge -> background path (needs an http origin)
    const server = createServer((req, res) => {
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end('<!doctype html><title>bridge</title><p>bridge</p>');
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port;
    const { targetId } = await browser.send('Target.createTarget', { url: `http://127.0.0.1:${port}/` });
    await sleep(2500);
    const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
    const pageTarget = targets.find((t) => t.id === targetId);
    if (!pageTarget) throw new Error('bridge tab not found');
    const tab = await connect(pageTarget.webSocketDebuggerUrl);
    await tab.send('Page.enable');
    await sleep(800);
    await tab.send('Runtime.evaluate', {
      expression: `(function(){window.__lyco=null;window.addEventListener('message',function(e){if(e.data&&e.data.source==='lycodl'&&e.data.type==='runAdapterResult')window.__lyco=e.data;});return 1;})()`,
      returnByValue: true,
    });
    await tab.send('Runtime.evaluate', {
      expression: `window.postMessage(${JSON.stringify({ source: 'lycodl', type: 'runAdapter', requestId: 'ci', adapter, payload: { file: '', fileName: '' } })}, '*')`,
      returnByValue: true,
    });
    let bridged = null;
    for (let i = 0; i < 20 && !bridged; i += 1) {
      await sleep(1500);
      const r = await tab.send('Runtime.evaluate', { expression: 'window.__lyco', returnByValue: true });
      if (r.result && r.result.value) bridged = r.result.value;
    }
    server.close();
    if (!bridged || !bridged.ok) throw new Error('page bridge failed: ' + JSON.stringify(bridged));
    console.log('bridge result', JSON.stringify(bridged.result));

    console.log('OK: extension executor, page bridge and manifest validated');
    browser.ws.close();
  } finally {
    chrome.kill('SIGKILL');
    await sleep(800);
    await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(() => {});
  }
}

main().catch((e) => {
  console.error('extension smoke failed:', e && e.message ? e.message : e);
  process.exit(1);
});