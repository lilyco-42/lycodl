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
// 6 个平台的真实提交器（CDP + 各站 DOM 流程）。可用 settings.publishPlatformCommand 覆盖。
const DEFAULT_PUBLISHER = '/home/lilyco/.local/share/lycodl/platform-publish.mjs';
const DEFAULT_PLATFORM_COMMAND = 'node ' + DEFAULT_PUBLISHER;

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

// 所有平台（含 x）统一交给 platform-publish.mjs：stdin 一行 JSON，stdout 一行 JSON
async function doPlatform(req, dest, platformCommand, share, passcode) {
  const cmd = platformCommand || DEFAULT_PLATFORM_COMMAND;
  const payload = Object.assign({}, req, { dest, shareUrl: share, sharePasscode: passcode });
  const timeout = Math.max(60, Number(req.timeoutSecs || 7200)) * 1000;
  const out = spawnSync('sh', ['-c', cmd], {
    input: JSON.stringify(payload), encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024,
  });
  if (out.error) return { dest, status: 'failed', detail: String(out.error).slice(0, 400) };
  const lines = String(out.stdout || '').trim().split('\n').filter(Boolean);
  const line = lines[lines.length - 1];
  if (!line) {
    return { dest, status: 'failed', detail: ('平台提交器无输出: ' + String(out.stderr || '')).slice(0, 400) };
  }
  try {
    const j = JSON.parse(line);
    const status = j.status === 'ok' ? 'ok' : (j.status === 'unavailable' ? 'unavailable' : 'failed');
    return { dest, status, detail: String(j.detail || line).slice(0, 400), url: j.url || null };
  } catch (e) { return { dest, status: 'failed', detail: ('平台提交器输出解析失败: ' + line).slice(0, 400) }; }
}

async function main(raw) {
  let req;
  try { req = JSON.parse(raw); } catch (e) { throw new Error('driver 输入不是合法 JSON: ' + e.message); }
  const pending = Array.isArray(req.pending) ? req.pending : [];
  const dry = !!req.dryRun;
  const share = req.shareUrl || DEFAULT_SHARE;
  const passcode = req.sharePasscode || DEFAULT_PASSCODE;
  const platformCommand = req.platformCommand || '';
  for (const dest of pending) {
    try {
      if (dry) { emit({ dest, status: 'dry-run', detail: '演练：' + (req.fileName || req.file) + ' 将发布到 ' + dest }); continue; }
      let r;
      if (dest === 'quark') r = await doQuark(req);
      else if (dest === 'x' || PLATFORMS.indexOf(dest) >= 0) r = await doPlatform(req, dest, platformCommand, share, passcode);
      else r = { dest, status: 'failed', detail: '未知目的地' };
      emit(r);
    } catch (e) { emit({ dest, status: 'failed', detail: String(e.message || e).slice(0, 400) }); }
  }
}
