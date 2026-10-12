# The TypeScript client against a node on KVM

**What this checks:** [`sdk/typescript`](../../../../sdk/typescript/README.md)
driving a real node, with nothing else. boat has its own Python and
TypeScript clients; see the [comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-typescript-sdk-kvm.mjs`, run with Node 20, starts an owned
`hv2-sandboxd` and imports the client from the checkout. Its five cases are
the [Python client's](../python-sdk-kvm/README.md), in the same order and with
the same assertions:

1. A machine; `exec` of a line that prints to both streams and exits 4; a
   run followed to its end.
2. 4,096 bytes of every byte value in and out; served by the guest's `httpd`
   through a published port; unpublished.
3. Stop, snapshot, read and list without booting, resize to 2 GiB and two
   processors the guest counts, restore to 1 GiB with the first note back,
   and a 1 GiB download under 256 MiB on disk.
4. An environment's variable and exact binary file in a new sandbox; usage
   read with it running.
5. 404, 409 and 400 thrown as `ApiError` with the node's words.

## Result

All five passed on the first run, on Linux 6.18.33 under WSL2 with nested
KVM, on 2026-10-12. [`report.json`](report.json) has the client's digest and
each refusal's words.

Separately, `tsc --strict` (TypeScript 5.6.3) compiles
`sdk/typescript/test/types.ts` against `index.d.ts`, including three calls
it must refuse; with one of those made valid, it fails, so the check is not
empty.

## What this does not show

- **Prompts** through a node; they are tested against the fixture only.
- **A control plane, keys and teams.** The node ran without keys.
- **Deno, Bun or a browser.** Only Node.
