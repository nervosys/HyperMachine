#!/usr/bin/env node
// The TypeScript client (sdk/typescript) against a real node on KVM.
//
// node tools/check-typescript-sdk-kvm.mjs --daemon D --kernel K --initrd I --output DIR
//
// Runs an owned hv2-sandboxd and drives it with nothing but the client: a
// machine made, a command run, a run followed, a file in and out, a port
// published and fetched, the machine stopped, resized, snapshotted, read
// without booting, restored and downloaded, an environment applied to a
// sandbox, usage read, and refusals thrown as ApiError. Writes report.json
// and the daemon's log into --output.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { ApiError, Client } from '../sdk/typescript/index.js';

const here = dirname(fileURLToPath(import.meta.url));
const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => (arg.startsWith('--') ? [...pairs, [arg.slice(2), all[i + 1]]] : pairs), []),
);
for (const need of ['daemon', 'kernel', 'initrd', 'output']) {
  if (!args[need]) throw new Error(`--${need} is required`);
}
const digest = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
const assert = (ok, what) => {
  if (!ok) throw new Error(`assertion failed: ${typeof what === 'string' ? what : JSON.stringify(what)}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () =>
  new Promise((done) => {
    const server = createServer();
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });

mkdirSync(args.output, { recursive: false });
const report = {
  success: false,
  purpose: 'functional verification of the TypeScript client against a node on KVM; no timing claim',
  node: process.version,
  input_sha256: Object.fromEntries(['daemon', 'kernel', 'initrd'].map((k) => [k, digest(args[k])])),
  sdk_sha256: digest(resolve(here, '../sdk/typescript/index.js')),
  cases: [],
};
report.input_sha256.driver = digest(fileURLToPath(import.meta.url));
const work = mkdtempSync('/var/tmp/hm-ts-sdk-check-');
const api = await freePort();
const log = createWriteStream(join(args.output, 'daemon.log'));
const daemon = spawn(
  args.daemon,
  ['--port', String(api), '--proxy-port', String(await freePort()), '--machine-dir', join(work, 'machines'),
   '--machine-ports', '33110-33119', '--memory-mb', '256', '--cpu-cores', '1', '--capacity', '4'],
  { env: { ...process.env, HV2_KERNEL: args.kernel, HV2_INITRD: args.initrd, RUST_LOG: 'info' }, stdio: ['ignore', 'pipe', 'pipe'] },
);
daemon.stdout.pipe(log);
daemon.stderr.pipe(log);
let exited = false;
daemon.on('exit', () => (exited = true));
const hm = new Client({ endpoint: `http://127.0.0.1:${api}`, timeoutMs: 300_000 });

async function step(name, action) {
  const row = { name, success: false };
  report.cases.push(row);
  const detail = await action();
  row.success = true;
  if (detail) row.detail = detail;
  console.log('ok:', name);
}

const state = {};
try {
  const deadline = Date.now() + 180_000;
  for (;;) {
    assert(!exited, 'the daemon exited');
    try {
      await hm.machines.list();
      break;
    } catch {
      assert(Date.now() < deadline, 'daemon readiness');
      await sleep(200);
    }
  }

  await step('a machine is made, a command run, a run followed', async () => {
    const web = await hm.machines.create('ts-01', { diskGiB: 1, memoryMB: 512, cpuCount: 1 });
    assert(web.state === 'running', web.info);
    state.web = web;
    const result = await web.exec('echo hello; echo oops >&2; exit 4');
    assert(result.exitCode === 4 && result.stdout === 'hello\n' && result.stderr === 'oops\n', result);
    const run = await web.runs.start('for i in 1 2 3; do echo line-$i; sleep 0.5; done');
    let text = '';
    for await (const chunk of run.follow({ intervalMs: 200 })) text += chunk;
    assert(text === 'line-1\nline-2\nline-3\n' && run.exitCode === 0, { text, info: run.info });
    return { machine: web.id };
  });

  await step('a file goes in and out, and a published port serves it', async () => {
    const web = state.web;
    const payload = Buffer.from(Array.from({ length: 4096 }, (_, i) => i % 256));
    await web.files.write('/root/www/data.bin', payload);
    assert((await web.files.read('/root/www/data.bin')).equals(payload), 'file read back');
    await web.runs.start('exec httpd -f -p 8080 -h /root/www');
    const until = Date.now() + 20_000;
    while (!(await web.exec('netstat -ltn')).stdout.includes('8080')) {
      assert(Date.now() < until, 'httpd listening');
      await sleep(200);
    }
    const published = await web.ports.publish(8080);
    const fetched = Buffer.from(await (await fetch(`http://127.0.0.1:${published.hostPort}/data.bin`)).arrayBuffer());
    assert(fetched.equals(payload), 'fetched through the published port');
    await web.ports.unpublish(published.hostPort);
    assert((await web.ports.list()).length === 0, 'no ports left');
    return { hostPort: published.hostPort };
  });

  await step('stopped, resized, snapshotted, read without booting, restored, downloaded sparse', async () => {
    const web = state.web;
    await web.exec('echo first > /root/note; sync');
    await web.stop();
    const snap = await web.snapshots.take('first');
    assert((await snap.read('/root/note')).toString() === 'first\n', 'note in the snapshot');
    assert((await snap.listdir('/root')).some((e) => e.name === 'note'), 'listed');
    await web.resize({ diskGiB: 2, cpuCount: 2 });
    await web.start();
    assert((await web.exec('grep -c ^processor /proc/cpuinfo')).stdout === '2\n', 'two processors');
    await web.exec('echo second > /root/note; sync');
    await web.stop();
    const restored = await snap.restore();
    assert(restored.info.diskGiB === 1, restored.info);
    await web.start();
    assert((await web.exec('cat /root/note')).stdout === 'first\n', 'note restored');
    const image = join(work, 'disk.img');
    assert((await snap.download(image)) === 1 << 30, 'a 1 GiB disk');
    const blocks = statSync(image).blocks * 512;
    assert(blocks < 256 << 20, `the download was not sparse: ${blocks} bytes on disk`);
    return { snapshot: snap.id, bytes_on_disk: blocks };
  });

  await step('an environment starts a sandbox, and its usage is read', async () => {
    await hm.putEnvironment('ts-env', { envVars: { GREETING: 'hi' }, files: { '/etc/sdk/flag': Buffer.from([0, 1, 115, 101, 116]) } });
    assert((await hm.environments()).some((e) => e.name === 'ts-env'), 'environment listed');
    const { sandboxID } = await hm.call('POST', '/sandboxes', { templateID: 'base', environment: 'ts-env' });
    const out = await hm.call('POST', `/sandboxes/${sandboxID}/exec`, { cmd: 'echo "$GREETING"; base64 /etc/sdk/flag' });
    // b"\x00\x01set", exactly.
    assert(out.stdout === 'hi\nAAFzZXQ=\n', out);
    await sleep(1000);
    const usage = await hm.usage({ by: 'sandbox' });
    const mine = usage.groups.find((g) => g.key === sandboxID);
    assert(mine && mine.runningSeconds > 0, usage);
    await hm.call('DELETE', `/sandboxes/${sandboxID}`);
    await hm.deleteEnvironment('ts-env');
    return { usage_seconds: mine.runningSeconds };
  });

  await step('refusals are thrown as ApiError with the node\'s words', async () => {
    const results = {};
    for (const [label, action, status] of [
      ['a machine that is not there', () => hm.machines.get('absent-99'), 404],
      ['a second machine of one name', () => hm.machines.create('ts-01', { diskGiB: 1 }), 409],
      ['a disk that would shrink', async () => { await state.web.stop(); await state.web.resize({ diskGiB: 0 }); }, 400],
    ]) {
      try {
        await action();
        throw new Error(`${label}: no refusal`);
      } catch (error) {
        assert(error instanceof ApiError && error.status === status, `${label}: ${error}`);
        results[label] = `${error.status} ${error.detail}`;
      }
    }
    await state.web.delete();
    return results;
  });

  report.success = true;
} finally {
  if (!exited) {
    daemon.kill('SIGTERM');
    for (let i = 0; i < 300 && !exited; i++) await sleep(100);
    if (!exited) daemon.kill('SIGKILL');
  }
  log.end();
  writeFileSync(join(args.output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  rmSync(work, { recursive: true, force: true });
  console.log('report:', join(args.output, 'report.json'));
}
