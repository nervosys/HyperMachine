// The client against an HTTP fixture that answers as a node does.

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inspect } from 'node:util';
import { after, before, beforeEach, test } from 'node:test';

import { ApiError, Client } from '../index.js';

const MACHINE = { machineID: 'vm-1', name: 'web-01', state: 'running' };
const seen = [];
const answers = new Map();
let server;
let hm;

before(async () => {
  server = createServer((request, response) => {
    const parts = [];
    request.on('data', (part) => parts.push(part));
    request.on('end', () => {
      const body = Buffer.concat(parts);
      seen.push({ method: request.method, url: request.url, headers: request.headers, body });
      const [status, payload] = answers.get(`${request.method} ${request.url}`) ?? [404, { code: 404, message: 'no route' }];
      const data = payload === null ? Buffer.alloc(0) : Buffer.isBuffer(payload) ? payload : Buffer.from(JSON.stringify(payload));
      response.writeHead(status, { 'content-length': data.length });
      response.end(data);
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
});

after(() => server.close());

beforeEach(() => {
  seen.length = 0;
  answers.clear();
  hm = new Client({ endpoint: `http://127.0.0.1:${server.address().port}`, apiKey: 'k-secret' });
});

const json = () => JSON.parse(seen.at(-1).body.toString());

async function webMachine() {
  answers.set('GET /machines/web-01', [200, MACHINE]);
  return hm.machines.get('web-01');
}

test('the key is sent as a header and shown nowhere', async () => {
  answers.set('GET /machines', [200, [MACHINE]]);
  const [machine] = await hm.machines.list();
  assert.equal(machine.name, 'web-01');
  assert.equal(seen.at(-1).headers['x-api-key'], 'k-secret');
  // Not among its own keys, so neither printed nor copied with them.
  assert.ok(!Object.keys(hm).includes('apiKey'));
  assert.ok(!inspect(hm, { depth: 4 }).includes('k-secret'));
  assert.ok(!String(hm).includes('k-secret'));
});

test('a refusal throws with the node s words', async () => {
  answers.set('POST /machines', [409, { code: 409, message: 'machine web-01 exists' }]);
  await assert.rejects(hm.machines.create('web-01'), (error) => {
    assert.ok(error instanceof ApiError);
    assert.equal(error.status, 409);
    assert.equal(error.detail, 'machine web-01 exists');
    return true;
  });
});

test('create sends only what was given, and a name stays one segment', async () => {
  answers.set('POST /machines', [201, MACHINE]);
  await hm.machines.create('web-01', { cpuCount: 2, diskGiB: 20, start: false });
  assert.deepEqual(json(), { name: 'web-01', start: false, cpuCount: 2, diskGiB: 20 });
  await hm.machines.create('ai-01', { image: 'noble.raw', guestAgent: true, agents: ['claude', 'codex'] });
  assert.deepEqual(json(), { name: 'ai-01', start: true, image: 'noble.raw', guestAgent: true, agents: ['claude', 'codex'] });
  answers.set('GET /machines/a%2F..%2Fb', [200, { ...MACHINE, name: 'a/../b' }]);
  const odd = await hm.machines.get('a/../b');
  assert.equal(seen.at(-1).url, '/machines/a%2F..%2Fb');
  answers.set('POST /machines/a%2F..%2Fb/stop', [200, { ...MACHINE, name: 'a/../b', state: 'stopped' }]);
  assert.equal((await odd.stop()).state, 'stopped');
});

test('exec and resize', async () => {
  const machine = await webMachine();
  answers.set('POST /machines/web-01/exec', [200, { exit_code: 3, signal: null, stdout: 'out\n', stderr: 'err\n' }]);
  assert.deepEqual(await machine.exec('false', { timeoutSecs: 5 }), { exitCode: 3, signal: null, stdout: 'out\n', stderr: 'err\n' });
  assert.deepEqual(json(), { cmd: 'false', timeout_secs: 5 });
  answers.set('PATCH /machines/web-01', [200, { ...MACHINE, cpuCount: 4 }]);
  await machine.resize({ cpuCount: 4 });
  assert.deepEqual(json(), { cpuCount: 4 });
});

test('a run is followed from where the last read stopped', async () => {
  const machine = await webMachine();
  answers.set('POST /machines/web-01/runs', [201, { runID: 7, state: 'running' }]);
  const run = await machine.runs.start('make', { cwd: '/srv', env: { CI: '1' } });
  assert.deepEqual(json(), { cmd: 'make', cwd: '/srv', env: { CI: '1' } });
  answers.set('GET /machines/web-01/runs/7?stdout=0&stderr=0', [200, { runID: 7, state: 'running', stdout: { text: 'a', next: 1 } }]);
  answers.set('GET /machines/web-01/runs/7?stdout=1&stderr=0', [200, { runID: 7, state: 'exited', exitCode: 0, stdout: { text: 'bc', next: 3 } }]);
  let text = '';
  for await (const chunk of run.follow({ intervalMs: 0 })) text += chunk;
  assert.equal(text, 'abc');
  assert.equal(run.exitCode, 0);
});

test('prompt events are read on from next', async () => {
  const machine = await webMachine();
  answers.set('POST /machines/web-01/prompts', [201, { promptID: 2, state: 'running' }]);
  const prompt = await machine.prompts.start('claude', 'fix it', { user: 'dev', unattended: true, env: { K: 'v' } });
  assert.deepEqual(json(), { provider: 'claude', prompt: 'fix it', user: 'dev', env: { K: 'v' }, unattended: true });
  answers.set('GET /machines/web-01/prompts/2?after=0', [200, { state: 'running', next: 5, events: [{ offset: 0, event: { type: 'a' } }] }]);
  answers.set('GET /machines/web-01/prompts/2?after=5', [200, { state: 'exited', next: 9, events: [{ offset: 5, text: 'plain' }] }]);
  const events = [];
  for await (const event of prompt.events({ intervalMs: 0 })) events.push(event);
  assert.deepEqual(events, [{ offset: 0, event: { type: 'a' } }, { offset: 5, text: 'plain' }]);
});

test('a conversation is spoken to and its turns are prompts', async () => {
  const machine = await webMachine();
  answers.set('POST /machines/web-01/conversations', [201, { conversationID: 3, name: 'fix', state: 'idle' }]);
  const talk = await machine.conversations.create('codex', { name: 'fix', user: 'dev', from: 1 });
  assert.deepEqual(json(), { provider: 'codex', name: 'fix', user: 'dev', from: '1' });
  answers.set('POST /machines/web-01/conversations/3/messages', [202, { conversationID: 3, promptID: 8, steered: true }]);
  const turn = await talk.say('and the docs', { env: { K: 'v' } });
  assert.deepEqual(json(), { prompt: 'and the docs', env: { K: 'v' } });
  assert.deepEqual([turn.id, turn.info.steered, turn.path], [8, true, '/machines/web-01/prompts/8']);
  answers.set('GET /machines/web-01/conversations/3/transcript', [200, [{ role: 'user', text: 'hi', promptID: 8 }]]);
  assert.equal((await talk.transcript())[0].role, 'user');
  answers.set('GET /machines/web-01/conversations/a%2Fb', [200, { conversationID: 4, name: 'a/b' }]);
  assert.equal((await machine.conversations.get('a/b')).id, 4);
});

test('files go in and out as bytes', async () => {
  const machine = await webMachine();
  answers.set('PUT /machines/web-01/files?path=%2Fa+b%2Fc&user=dev', [204, null]);
  await machine.files.write('/a b/c', Buffer.from([0, 255, 100]), { user: 'dev' });
  assert.deepEqual([...seen.at(-1).body], [0, 255, 100]);
  answers.set('GET /machines/web-01/files?path=%2Fetc%2Fhostname', [200, Buffer.from('web-01\n\0')]);
  assert.deepEqual(await machine.files.read('/etc/hostname'), Buffer.from('web-01\n\0'));
});

test('a snapshot is read and downloaded with holes, never over a file', async () => {
  const machine = await webMachine();
  answers.set('POST /machines/web-01/snapshots', [201, { snapshotID: 'snap-1', name: 'x' }]);
  const snap = await machine.snapshots.take('x');
  answers.set('GET /machines/web-01/snapshots/snap-1/files?path=%2Fetc', [200, { entries: [{ name: 'hostname', type: 'file' }] }]);
  assert.equal((await snap.listdir('/etc'))[0].name, 'hostname');
  answers.set('GET /machines/web-01/snapshots/snap-1/file?path=%2Fetc%2Fhostname&partition=1', [200, Buffer.from('web-01\n')]);
  assert.deepEqual(await snap.read('/etc/hostname', { partition: 1 }), Buffer.from('web-01\n'));
  const disk = Buffer.concat([Buffer.from('head'), Buffer.alloc(3 << 20), Buffer.from('tail')]);
  answers.set('GET /machines/web-01/snapshots/snap-1/disk', [200, disk]);
  const scratch = mkdtempSync(join(tmpdir(), 'hm-ts-sdk-'));
  try {
    const target = join(scratch, 'disk.img');
    assert.equal(await snap.download(target), disk.length);
    assert.ok(readFileSync(target).equals(disk));
    await assert.rejects(snap.download(target), { code: 'EEXIST' });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('usage and environments', async () => {
  answers.set('GET /usage?from=2026-10-01T00%3A00%3A00Z&by=team', [200, { total: { sandboxes: 1 } }]);
  assert.equal((await hm.usage({ from: '2026-10-01T00:00:00Z', by: 'team' })).total.sandboxes, 1);
  answers.set('PUT /environments/web', [201, { name: 'web' }]);
  await hm.putEnvironment('web', { envVars: { A: '1' }, files: { '/etc/x': Buffer.from([0, 98, 105, 110]) } });
  assert.deepEqual(json(), { envVars: { A: '1' }, files: [{ path: '/etc/x', contentBase64: Buffer.from([0, 98, 105, 110]).toString('base64') }] });
});
