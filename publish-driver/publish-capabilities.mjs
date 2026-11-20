#!/usr/bin/env node
// lycodl-publish-capabilities-version: 1
//
// Read-only preflight for the publish UI. Never uploads or posts anything.
// stdin: { file, fileName, size, cdpUrl, quarkDir, kuakeCommand, platformCommand, targets }
// stdout: one JSON object: { caps: [{dest,available,published,reason,detail,source}], checkedAt, errors }
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const DESTS = ['quark', 'bilibili', 'douyin', 'xhs', 'kuaishou', 'youtube', 'x'];
const KUAKE_DEFAULT = '/home/lilyco/.config/kuake/run.sh';
const INSTALLED_PUBLISHER = '/home/lilyco/.local/share/lycodl/platform-publish.mjs';
const SIBLING_PUBLISHER = fileURLToPath(new URL('./platform-publish.mjs', import.meta.url));

// Cookie names that reliably mean a signed-in browser session for each platform.
// We check only names + whether expired; values are never logged or returned.
const COOKIE_RULES = {
  bilibili: [['SESSDATA', '.bilibili.com'], ['DedeUserID', '.bilibili.com']],
  douyin: [['sessionid', '.douyin.com'], ['sid_tt', '.douyin.com']],
  xhs: [['web_session', '.xiaohongshu.com']],
  kuaishou: [['userId', '.kuaishou.com'], ['passToken', 'id.kuaishou.com']],
  youtube: [['LOGIN_INFO', '.youtube.com'], ['SID', '.youtube.com']],
  x: [['auth_token', '.x.com'], ['ct0', '.x.com']],
};

function readStdin() {
  return new Promise((resolve, reject) => {
    let s = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', c => { s += c; });
    process.stdin.on('end', () => resolve(s));
    process.stdin.on('error', reject);
  });
}

function truncateBytes(s, n) {
  const b = Buffer.from(String(s || ''), 'utf8');
  return b.length <= n ? String(s || '') : b.subarray(0, n).toString('utf8');
}

function lastJson(text) {
  const raw = String(text || '').trim();
  if (!raw) return null;
  // kuake prints pretty-printed JSON; parse the whole document first.
  try { return JSON.parse(raw); } catch { /* look for a JSON suffix */ }
  const lines = raw.split(/\r?\n/);
  for (let i = lines.length - 1; i >= 0; i--) {
    const l = lines[i].trim();
    if (!l.startsWith('{') && !l.startsWith('[')) continue;
    try { return JSON.parse(lines.slice(i).join('\n')); } catch { /* keep looking */ }
  }
  return null;
}

async function httpJson(url, timeoutMs = 4000) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), timeoutMs);
  try {
    const r = await fetch(url, { signal: ctl.signal });
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return await r.json();
  } finally { clearTimeout(t); }
}

async function cdpCookies(cdpUrl) {
  const base = String(cdpUrl || 'http://127.0.0.1:9333').replace(/\/+$/, '');
  const ver = await httpJson(base + '/json/version');
  if (!ver || !ver.webSocketDebuggerUrl) throw new Error('CDP 未就绪');
  const ws = new WebSocket(ver.webSocketDebuggerUrl);
  const pending = new Map();
  let seq = 0;
  const ready = new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error('CDP WebSocket 连接超时')), 8000);
    ws.addEventListener('open', () => { clearTimeout(t); resolve(); }, { once: true });
    ws.addEventListener('error', () => { clearTimeout(t); reject(new Error('CDP WebSocket 连接失败')); }, { once: true });
  });
  ws.addEventListener('message', ev => {
    try {
      const m = JSON.parse(String(ev.data));
      if (!m || !m.id || !pending.has(m.id)) return;
      const { resolve, reject, timer } = pending.get(m.id);
      clearTimeout(timer);
      pending.delete(m.id);
      if (m.error) reject(new Error(String(m.error.message || 'CDP 错误')));
      else resolve(m.result);
    } catch { /* ignore */ }
  });
  await ready;
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++seq;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error('CDP 超时: ' + method));
    }, 10000);
    pending.set(id, { resolve, reject, timer });
    ws.send(JSON.stringify({ id, method, params }));
  });
  try {
    const r = await call('Storage.getCookies');
    return Array.isArray(r && r.cookies) ? r.cookies : [];
  } finally {
    try { ws.close(); } catch { /* ignore */ }
  }
}

function cookieOk(cookies, name, domain) {
  const now = Date.now() / 1000;
  return cookies.some(c => {
    const d = String(c.domain || '');
    const dm = d === domain || d === domain.replace(/^\./, '') || d.endsWith(domain);
    const live = c.expires === undefined || c.expires === null || c.expires < 0 || c.expires > now;
    return c.name === name && dm && live && String(c.value || '').length > 0;
  });
}

function kuakeJson(args, cmd) {
  const r = spawnSync(cmd, args, {
    encoding: 'utf8',
    timeout: 15000,
    maxBuffer: 32 * 1024 * 1024,
    env: process.env,
  });
  if (r.error) throw r.error;
  const j = lastJson(r.stdout);
  if (!j || j.success !== true) throw new Error(String((j && j.message) || (r.stderr || 'kuake 返回失败')).slice(0, 200));
  return j;
}

function publisherPresent(req) {
  if (req.platformCommand && String(req.platformCommand).trim()) {
    const first = String(req.platformCommand).trim().split(/\s+/)[0];
    if (first === 'node' || first === '/usr/bin/node' || first === 'nodejs') {
      const m = String(req.platformCommand).match(/(?:^|\s)([^\s]+\.mjs)(?=\s|$)/);
      return !m || existsSync(m[1]);
    }
    return true;
  }
  return existsSync(SIBLING_PUBLISHER) || existsSync(INSTALLED_PUBLISHER);
}

async function main(raw) {
  let req = {};
  try { req = JSON.parse(raw || '{}'); } catch { req = {}; }
  const errors = [];
  const fileName = String(req.fileName || req.file || '');
  const remoteName = truncateBytes(fileName, 120);
  const size = Number(req.size || 0);
  const targets = Array.isArray(req.targets) && req.targets.length
    ? DESTS.filter(d => req.targets.includes(d)) : DESTS.slice();

  let cookies = [];
  let cdpReady = false;
  let cdpDetail = '';
  try {
    cookies = await cdpCookies(req.cdpUrl);
    cdpReady = true;
    cdpDetail = 'CDP 已就绪，登录 Cookie ' + cookies.length + ' 项';
  } catch (e) {
    cdpDetail = String((e && e.message) || e).slice(0, 180);
    errors.push('CDP cookie 检查失败: ' + cdpDetail);
  }

  let quarkAvailable = false;
  let quarkPublished = false;
  let quarkDetail = '';
  try {
    const cmd = String(req.kuakeCommand || KUAKE_DEFAULT);
    if (!existsSync(cmd)) throw new Error('kuake 命令不存在: ' + cmd);
    const u = kuakeJson(['user'], cmd);
    quarkAvailable = true;
    quarkDetail = '夸克账号 ' + String((u.data && u.data.nickname) || '已登录');
    const qdir = String(req.quarkDir || '/06_待整理/lycodl-短剧集');
    const list = kuakeJson(['list', qdir], cmd);
    const rows = list && list.data && Array.isArray(list.data.list) ? list.data.list : [];
    quarkPublished = rows.some(x => String(x.file_name || '') === remoteName && Number(x.size || 0) === size && Number(x.size || 0) > 0);
    if (quarkPublished) quarkDetail += '；网盘已有同名同大小文件';
    else quarkDetail += '；网盘列表 ' + rows.length + ' 项';
  } catch (e) {
    quarkAvailable = false;
    quarkDetail = String((e && e.message) || e).slice(0, 250);
    errors.push('夸克检查失败: ' + quarkDetail);
  }

  const pubPresent = publisherPresent(req);
  if (!pubPresent) errors.push('平台发布器 platform-publish.mjs 不存在');
  const caps = targets.map(dest => {
    if (dest === 'quark') {
      return {
        dest,
        available: quarkAvailable,
        published: quarkPublished,
        reason: !quarkAvailable ? 'unavailable' : (quarkPublished ? 'published' : 'ready'),
        detail: quarkDetail,
        source: 'kuake-user-list',
      };
    }
    const rules = COOKIE_RULES[dest] || [];
    const missing = rules.filter(([n, d]) => !cookieOk(cookies, n, d)).map(([n]) => n);
    const cookieOkAll = rules.length > 0 && missing.length === 0;
    const available = cookieOkAll && pubPresent;
    const detail = !cookieOkAll
      ? ('缺少登录 Cookie: ' + (missing.join(', ') || '未配置规则'))
      : (!pubPresent ? '缺少平台发布器' : 'CDP 登录态可用');
    return {
      dest,
      available,
      published: false, // account-level duplication is merged from local ledger in Rust
      reason: !available ? 'unavailable' : 'ready',
      detail,
      source: 'cdp-cookies+publisher',
    };
  });
  process.stdout.write(JSON.stringify({
    caps,
    cdpReady,
    cdpDetail,
    cdpUrl: String(req.cdpUrl || 'http://127.0.0.1:9333'),
    checkedAt: Date.now(),
    errors,
  }) + '\n');
}

readStdin().then(main).catch(e => {
  process.stdout.write(JSON.stringify({
    caps: DESTS.map(dest => ({ dest, available: false, published: false, reason: 'unavailable', detail: String((e && e.message) || e), source: 'probe-error' })),
    checkedAt: Date.now(),
    errors: [String((e && e.message) || e)],
  }) + '\n');
});
