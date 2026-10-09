// Against a real `hm`: these start the binary and run real programs under it.
//
// HM_BIN names the binary. Without it there is nothing to test, and the
// suite fails instead of skipping: a suite that passes by not running is how
// a broken client would ship.

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { run, SandboxError, SandboxRefused, VERSION } from '../index.js';

const hm = process.env.HM_BIN;
assert.ok(hm && existsSync(hm), 'set HM_BIN to a built hm binary');

const windows = process.platform === 'win32';

/** A shell line, as this platform runs one, with the host's network so the
 *  request asks for nothing a test machine may be unable to enforce. */
function shell(line, extra = {}) {
  return windows
    ? {
        command: ['C:\\Windows\\System32\\cmd.exe', '/c', line],
        env: { SystemRoot: 'C:\\Windows', PATH: 'C:\\Windows\\System32' },
        network: { egress: 'host' },
        ...extra,
      }
    : {
        command: ['/bin/sh', '-c', line],
        env: { PATH: '/usr/bin:/bin' },
        network: { egress: 'host' },
        ...extra,
      };
}

test('a run resolves with the exit code and both streams', async () => {
  const line = windows ? 'echo out& echo err 1>&2& exit 3' : 'echo out; echo err >&2; exit 3';
  const result = await run(shell(line), { hm });
  assert.equal(result.version, VERSION);
  assert.equal(result.exitCode, 3);
  assert.equal(result.stdout.trim(), 'out');
  assert.equal(result.stderr.trim(), 'err');
  assert.equal(result.killedBy, null);
  assert.deepEqual(result.unenforced, []);
  assert.equal(result.backend, 'process');
  assert.equal(result.stdoutBytes.toString('utf8'), result.stdout);
  assert.ok(Array.isArray(result.controls));
});

test('standard input reaches the workload, and its environment is only the request\'s', async () => {
  process.env.HM_NODE_SDK_CANARY = 'leaked';
  const filter = windows ? 'findstr x' : 'grep x';
  const read = await run(shell(filter, { stdin: 'axb\nnone\n' }), { hm });
  assert.equal(read.stdout.trim(), 'axb');

  const show = windows ? 'echo [%HM_NODE_SDK_CANARY%][%GIVEN%]' : 'echo "[$HM_NODE_SDK_CANARY][$GIVEN]"';
  const request = shell(show);
  request.env.GIVEN = 'yes';
  const seen = await run(request, { hm });
  assert.equal(seen.stdout.trim(), windows ? '[%HM_NODE_SDK_CANARY%][yes]' : '[][yes]');
});

test('a workload past its deadline is killed, and that is a run that happened', async () => {
  const forever = windows ? 'for /l %i in () do @rem' : 'sleep 30';
  const result = await run(shell(forever, { limits: { timeoutMs: 500 } }), { hm });
  assert.equal(result.killedBy, 'wall-clock deadline');
});

test('output that is not UTF-8 comes back whole as bytes', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'hm-node-sdk-'));
  try {
    const file = join(directory, 'raw.bin');
    writeFileSync(file, Buffer.from([0x61, 0xff, 0xfe, 0x62]));
    const result = await run(shell(windows ? `type ${file}` : `cat '${file}'`), { hm });
    assert.deepEqual([...result.stdoutBytes], [0x61, 0xff, 0xfe, 0x62]);
    assert.equal(result.stdout, 'a\ufffd\ufffdb');
    assert.equal(result.stdoutBase64, undefined);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('a request that does not run rejects with why, and nothing ran', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'hm-node-sdk-'));
  const marker = join(directory, 'ran');
  try {
    const touch = windows ? `echo x> ${marker}` : `echo x > '${marker}'`;
    const cases = [
      [{ ...shell(touch), netwrok: {} }, 'invalid', 'netwrok'],
      [{ ...shell(touch), version: 9 }, 'invalid', 'version 9'],
      [shell(touch, { filesystem: { readOnly: ['relative/path'] } }), 'invalid', 'granted path'],
      [{ command: ['/no/such/program-anywhere'], network: { egress: 'host' } }, 'spawn', 'program-anywhere'],
    ];
    for (const [request, kind, says] of cases) {
      await assert.rejects(run(request, { hm }), (error) => {
        assert.ok(error instanceof SandboxRefused, String(error));
        assert.ok(error instanceof SandboxError);
        assert.equal(error.kind, kind);
        assert.ok(error.message.includes(says), error.message);
        assert.equal(error.response.version, VERSION);
        return true;
      });
    }
    assert.ok(!existsSync(marker), 'a refused request ran its program');

    // The same line does run when nothing is wrong with the request, so its
    // not having run above is the refusal and not a broken command.
    await run(shell(touch), { hm });
    assert.ok(existsSync(marker));
    assert.ok(readFileSync(marker, 'utf8').startsWith('x'));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('no network is enforced or refused, never dropped quietly', async () => {
  const request = shell('echo ran', { network: { egress: 'deny' } });
  try {
    const result = await run(request, { hm });
    assert.deepEqual(result.unenforced, []);
    assert.ok(result.controls.some((c) => c.control === 'network isolation' && c.enforced === true));
  } catch (error) {
    assert.ok(error instanceof SandboxRefused, String(error));
    assert.equal(error.kind, 'unsupported');
    assert.ok(error.message.includes('network isolation'), error.message);
  }
});

test('a binary that is not there, or is not hm, is an error that says so', async () => {
  await assert.rejects(run(shell('echo x'), { hm: join(tmpdir(), 'no-such-hm-binary') }), (error) => {
    assert.ok(error instanceof SandboxError && !(error instanceof SandboxRefused), String(error));
    assert.ok(error.message.includes('could not start'), error.message);
    return true;
  });
  // Node itself, asked to be hm: it exits non-zero without a JSON response.
  await assert.rejects(run(shell('echo x'), { hm: process.execPath }), (error) => {
    assert.ok(error instanceof SandboxError && !(error instanceof SandboxRefused), String(error));
    assert.ok(error.message.includes('without a response'), error.message);
    return true;
  });
});

test('the types name every field the schema does', () => {
  const schema = JSON.parse(
    readFileSync(new URL('../../../docs/schemas/sandbox-request-v1.schema.json', import.meta.url), 'utf8'),
  );
  const types = readFileSync(new URL('../index.d.ts', import.meta.url), 'utf8');
  const request = types.slice(types.indexOf('export interface Request'), types.indexOf('export interface ControlReport'));
  const names = (properties) =>
    Object.entries(properties).flatMap(([name, value]) => [name, ...names(value.properties ?? {})]);
  const wanted = names(schema.properties);
  assert.ok(wanted.includes('denied') && wanted.includes('timeoutMs'), wanted.join());
  for (const name of wanted) {
    assert.match(request, new RegExp(`\\b${name}\\??:`), `index.d.ts has no ${name}`);
  }
});
