# HyperMachine sandbox for Node

Run a program in HyperMachine's [process sandbox](../../docs/SANDBOXES.md) from
Node.

```js
import { run, SandboxRefused } from '@nervosys/hypermachine-sandbox';

const result = await run({
  command: ['/usr/bin/python3', '-c', 'print(6 * 7)'],
  env: { PATH: '/usr/bin' },
  limits: { memoryBytes: 256 * 1024 * 1024, timeoutMs: 10_000 },
  filesystem: { confine: true, readOnly: ['/usr', '/lib', '/lib64'] },
});

result.exitCode;   // 0
result.stdout;     // '42\n'
result.killedBy;   // null, or the limit that killed it
```

## What this is

A client of `hm sandbox exec`. It writes one JSON request to the `hm` binary
and reads one JSON response. The containment is `hm`'s, and so is the report of
what this host could and could not enforce; this package adds nothing to either
and has no dependencies.

So it needs `hm`: on `PATH`, named by `HM_BIN`, or passed as `{ hm: '/path/to/hm' }`.
It is not bundled.

The package is not on npm. Use it from a checkout:

```sh
npm install /path/to/HyperMachine/sdk/node
```

## What comes back

`run` resolves for every run that happened, whatever the workload did. A
non-zero exit is in `exitCode`; a workload killed at its deadline has
`killedBy: 'wall-clock deadline'`. Output is in `stdout` and `stderr` as text,
and in `stdoutBytes` and `stderrBytes` exactly as written, for output that is
not UTF-8.

It rejects in two ways:

- **`SandboxRefused`**: the run did not happen and nothing ran. `kind` is
  `invalid` (not a request this version reads), `unsupported` (it asks for a
  control this host cannot enforce), `spawn` (the program could not be
  started), `confinement` or `runtime`.
- **`SandboxError`**: `hm` could not be started, or did not answer.

## The defaults are the careful ones

They are `hm`'s, not this package's:

- No `network` means no network. `network: { allow: ['example.com'] }` lets
  the workload reach those hosts and no others, through a proxy `hm` runs for
  it; Linux and macOS.
- No `bestEffort` means a request this host cannot enforce is refused. With it,
  the run goes ahead and `unenforced` lists what was dropped.
- The workload's environment is exactly `env`. Nothing of Node's is inherited,
  so a request with no `PATH` has none.
- A field the format does not have is an error, not something skipped.

The request is typed in [`index.d.ts`](index.d.ts); the format itself is
[`sandbox-request-v1.schema.json`](../../docs/schemas/sandbox-request-v1.schema.json).
What each platform enforces is in [`docs/SANDBOXES.md`](../../docs/SANDBOXES.md).

## Not here

- No streaming: output arrives when the run ends. `hm sandbox run` streams.
- No cancellation from the caller. Give the run a `timeoutMs`.
- No microVM backend: `hm sandbox exec` runs the process sandbox.

## Tests

```sh
HM_BIN=/path/to/hm npm test
```

They start the real binary and run real programs under it, and fail without
`HM_BIN` instead of skipping.
