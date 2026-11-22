#!/usr/bin/env node
// Static validation for the MV3 extension (runs on Linux/macOS/Windows CI).
// Usage: node extension/validate-extension.mjs
'use strict';

import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const DIR = new URL('./', import.meta.url);
const errors = [];

function must(cond, msg) {
  if (!cond) errors.push(msg);
}

let manifest;
try {
  manifest = JSON.parse(readFileSync(fileURLToPath(new URL('manifest.json', DIR)), 'utf8'));
} catch (e) {
  console.error('cannot read manifest.json: ' + e.message);
  process.exit(1);
}

must(manifest.manifest_version === 3, 'manifest_version must be 3');
must(typeof manifest.name === 'string' && manifest.name.length > 0, 'name missing');
must(/^\d+\.\d+\.\d+$/.test(String(manifest.version || '')), 'version must be semver');

const perms = manifest.permissions || [];
for (const p of ['debugger', 'tabs', 'storage']) {
  must(perms.includes(p), `missing permission: ${p}`);
}

const files = [];
if (manifest.background && manifest.background.service_worker) files.push(manifest.background.service_worker);
for (const cs of manifest.content_scripts || []) {
  for (const js of cs.js || []) files.push(js);
}
for (const icon of Object.values(manifest.icons || {})) files.push(icon);

for (const f of files) {
  must(existsSync(fileURLToPath(new URL(f, DIR))), `missing file referenced by manifest: ${f}`);
}

must(files.includes('page-bridge.js'), 'page-bridge.js must be a content script');

const bgFile = manifest.background && manifest.background.service_worker;
const bg = bgFile ? readFileSync(fileURLToPath(new URL(bgFile, DIR)), 'utf8') : '';
must(bg.includes('importScripts("adapter-executor.js")'), 'background must importScripts adapter-executor.js');

const ext = manifest.externally_connectable || {};
must(Array.isArray(ext.matches) && ext.matches.some((m) => m.includes('127.0.0.1')), 'externally_connectable must allow 127.0.0.1');

const bridge = readFileSync(fileURLToPath(new URL('page-bridge.js', DIR)), 'utf8');
must(bridge.includes('runAdapterResult'), 'page-bridge must answer runAdapterResult');
must(bridge.includes('postMessage'), 'page-bridge must use postMessage');

if (errors.length) {
  console.error('extension validation failed:');
  for (const e of errors) console.error('  - ' + e);
  process.exit(1);
}
console.log(`extension ok: v${manifest.version}, ${files.length} referenced files`);