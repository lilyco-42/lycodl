#!/usr/bin/env node
// Validate every adapter against the v1 contract without third-party deps.
// Usage: node publish-driver/validate-adapters.mjs [--strict]
'use strict';

import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const DIR = fileURLToPath(new URL('./adapters/', import.meta.url));
const ACTIONS = ['goto', 'waitFor', 'waitMs', 'setFile', 'fill', 'clickText', 'clickDeep', 'eval', 'press'];
const REQUIRED = {
  goto: ['url'],
  waitFor: ['expr'],
  waitMs: ['ms'],
  setFile: ['selector'],
  fill: ['selector', 'value'],
  clickText: ['texts'],
  clickDeep: ['texts'],
  eval: ['expr'],
  press: [],
};

function fail(list, file, msg) {
  list.push(`${file}: ${msg}`);
}

export function validateAdapter(file, a, errors) {
  if (!a || typeof a !== 'object') return fail(errors, file, 'not an object');
  for (const k of ['id', 'name', 'origin', 'steps', 'result']) {
    if (a[k] === undefined) fail(errors, file, `missing ${k}`);
  }
  if (a.id && !/^[a-z0-9][a-z0-9_-]{0,31}$/.test(a.id)) fail(errors, file, `bad id: ${a.id}`);
  if (a.id && file !== a.id + '.json') fail(errors, file, `file name must be ${a.id}.json`);
  if (a.origin && !/^https:\/\//.test(a.origin)) fail(errors, file, 'origin must be https');
  if (!Array.isArray(a.steps) || !a.steps.length) fail(errors, file, 'steps must be a non-empty array');
  for (const [i, s] of (a.steps || []).entries()) {
    if (!s || !ACTIONS.includes(s.action)) {
      fail(errors, file, `step ${i}: bad action ${s && s.action}`);
      continue;
    }
    for (const k of REQUIRED[s.action]) {
      if (s[k] === undefined) fail(errors, file, `step ${i} (${s.action}) missing ${k}`);
    }
    if (s.action === 'fill' && s.value !== undefined && !String(s.value).startsWith('$') && !['$file', '$fileName', '$title', '$desc'].includes(String(s.value))) {
      fail(errors, file, `step ${i}: fill value should be a $variable or literal`);
    }
    if (s.texts && !Array.isArray(s.texts)) fail(errors, file, `step ${i}: texts must be an array`);
  }
  const r = a.result || {};
  const ok = r.success && (r.success.expr || r.success.texts || r.success.urlIncludes);
  if (!ok) fail(errors, file, 'result.success needs expr, texts or urlIncludes');
  if (a.cookies && !Array.isArray(a.cookies)) fail(errors, file, 'cookies must be an array');
}

export function validateAll() {
  const errors = [];
  const files = readdirSync(DIR).filter((f) => f.endsWith('.json') && f !== 'schema.json');
  if (!files.length) errors.push('no adapters found');
  for (const f of files) {
    let a;
    try {
      a = JSON.parse(readFileSync(new URL(`./adapters/${f}`, import.meta.url), 'utf8'));
    } catch (e) {
      fail(errors, f, 'invalid JSON: ' + e.message);
      continue;
    }
    validateAdapter(f, a, errors);
  }
  return { files, errors };
}

const isCli = process.argv[1] && process.argv[1].endsWith('validate-adapters.mjs');
if (isCli) {
  const { files, errors } = validateAll();
  if (errors.length) {
    console.error('adapter validation failed:');
    for (const e of errors) console.error('  - ' + e);
    process.exit(1);
  }
  console.log(`adapters ok: ${files.length} (${files.map((f) => f.replace(/\.json$/, '')).join(', ')})`);
}