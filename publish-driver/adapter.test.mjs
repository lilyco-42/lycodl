import test from 'node:test';
import assert from 'node:assert/strict';
import { loadAdapter, listAdapters } from './adapter-runner.mjs';
import { validateAdapter, validateAll } from './validate-adapters.mjs';

test('every shipped adapter passes the v1 contract', () => {
  const { errors } = validateAll();
  assert.deepEqual(errors, []);
});

test('adapter registry discovers at least the three shipped platforms', () => {
  const ids = listAdapters();
  for (const id of ['xhs', 'zhihu', 'weibo']) {
    assert.ok(ids.includes(id), `missing adapter ${id}`);
  }
});

test('xhs adapter keeps its publish step and success test', () => {
  const a = loadAdapter('xhs');
  assert.equal(a.id, 'xhs');
  assert.ok(a.steps.some((s) => s.action === 'setFile'));
  assert.ok(a.steps.some((s) => s.action === 'clickDeep'));
  assert.ok((a.result.success.texts || []).includes('发布成功'));
});

test('validator rejects missing required fields', () => {
  const errors = [];
  validateAdapter('bad.json', { id: 'bad', name: 'x' }, errors);
  assert.ok(errors.some((e) => e.includes('missing origin')));
  assert.ok(errors.some((e) => e.includes('missing steps')));
});

test('validator rejects unknown actions and non-https origins', () => {
  const errors = [];
  validateAdapter('bad.json', {
    id: 'bad',
    name: 'x',
    origin: 'http://example.com',
    steps: [{ action: 'teleport' }],
    result: { success: { expr: 'true' } },
  }, errors);
  assert.ok(errors.some((e) => e.includes('bad action')));
  assert.ok(errors.some((e) => e.includes('origin must be https')));
});

test('validator requires a success probe', () => {
  const errors = [];
  validateAdapter('nope.json', {
    id: 'nope',
    name: 'x',
    origin: 'https://example.com',
    steps: [{ action: 'waitMs', ms: 1 }],
    result: {},
  }, errors);
  assert.ok(errors.some((e) => e.includes('result.success needs')));
});