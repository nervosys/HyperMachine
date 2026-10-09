// HyperMachine's process sandbox, from Node.
//
// This is a client of `hm sandbox exec`: it writes one JSON request to the
// program's standard input and reads one JSON response from its standard
// output. Nothing is contained by this file. The `hm` binary does that, and
// reports what it could and could not enforce; this passes the report on.

import { spawn } from 'node:child_process';

/** The request format this client writes. */
export const VERSION = 1;

/** What `hm sandbox exec` exits with when the run did not happen. */
const REFUSED = 2;

/** `hm` could not be run, or did not answer as `hm sandbox exec` does. */
export class SandboxError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = 'SandboxError';
  }
}

/**
 * The run did not happen, and `kind` says why: `invalid` (not a request this
 * version reads), `unsupported` (it asks for a control this host cannot
 * enforce), `spawn` (the program could not be started), `confinement` or
 * `runtime`. Nothing ran.
 */
export class SandboxRefused extends SandboxError {
  constructor(response) {
    super(response.error.message);
    this.name = 'SandboxRefused';
    this.kind = response.error.kind;
    this.response = response;
  }
}

/**
 * Run one request and resolve with what happened.
 *
 * A workload that exits non-zero, or is killed at its deadline, is a run
 * that happened: the promise resolves, and `exitCode` and `killedBy` say so.
 * It rejects with `SandboxRefused` when the run did not happen, and with
 * `SandboxError` when `hm` itself could not be used.
 *
 * `options.hm` is the `hm` binary: by default `HM_BIN`, then `hm` on `PATH`.
 */
export function run(request, options = {}) {
  const hm = options.hm ?? process.env.HM_BIN ?? 'hm';
  const body = JSON.stringify({ version: VERSION, ...request });
  return new Promise((resolve, reject) => {
    let child;
    try {
      // The environment is passed through for `hm` itself. The workload gets
      // none of it: its environment is exactly the request's `env`.
      child = spawn(hm, ['sandbox', 'exec'], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    } catch (cause) {
      reject(new SandboxError(`could not start ${hm}: ${cause.message}`, { cause }));
      return;
    }
    const out = [];
    const err = [];
    child.stdout.on('data', (chunk) => out.push(chunk));
    child.stderr.on('data', (chunk) => err.push(chunk));
    child.on('error', (cause) => {
      reject(new SandboxError(`could not start ${hm}: ${cause.message}`, { cause }));
    });
    // `hm` reads the whole request before it answers, so a write that fails
    // is one the `error` or `close` handler already explains.
    child.stdin.on('error', () => {});
    child.stdin.end(body);
    child.on('close', (code) => {
      const text = Buffer.concat(out).toString('utf8');
      let response;
      try {
        response = JSON.parse(text);
      } catch {
        const said = Buffer.concat(err).toString('utf8').trim() || text.trim() || 'nothing';
        reject(new SandboxError(`${hm} sandbox exec exited ${code} without a response: ${said}`));
        return;
      }
      if (code === REFUSED && response?.error) {
        reject(new SandboxRefused(response));
        return;
      }
      if (code !== 0 || response === null || typeof response !== 'object' || response.error) {
        reject(new SandboxError(`${hm} sandbox exec exited ${code} with an unexpected response: ${text.trim()}`));
        return;
      }
      resolve(resultOf(response));
    });
  });
}

/** The response, with output that was not UTF-8 given back as bytes. */
function resultOf(response) {
  const result = { ...response };
  for (const stream of ['stdout', 'stderr']) {
    const encoded = response[`${stream}Base64`];
    delete result[`${stream}Base64`];
    // Text when it was text; the lossy text and the exact bytes when not.
    result[`${stream}Bytes`] =
      encoded === undefined ? Buffer.from(response[stream] ?? '', 'utf8') : Buffer.from(encoded, 'base64');
  }
  return result;
}
