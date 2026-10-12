# The Python SDK against a node on KVM

**What this checks:** [`sdk/python`](../../../../sdk/python/README.md)
driving a real node, with nothing else: no `curl`, no `hm`. boat has its own
Python and TypeScript clients; see the [comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-python-sdk-kvm.py` runs an owned `hv2-sandboxd` with a
template, a machine directory and ten machine ports, and imports
`hypermachine` from the checkout.

1. **A machine, a command, a run.** `machines.create`; `exec` of a line that
   prints to both streams and exits 4, which must come back as exactly that;
   a run of three lines followed with `follow()` to its end and exit code 0.
2. **Files and a port.** 4,096 bytes of every byte value written with
   `files.write` and read back with `files.read`; the guest serving them with
   `httpd`; `ports.publish(8080)` and the file fetched from the node's port;
   `unpublish` and an empty `ports.list()`.
3. **Stop, snapshot, resize, restore, download.** A note written; stopped;
   a snapshot taken, its note read with `read` and its directory with
   `listdir`, without booting; resized to 2 GiB and two processors, which
   the guest must count; the note changed; stopped and `restore`d, which must
   give the 1 GiB disk back; started, the note is the first again; the disk
   downloaded with `download`, 1 GiB long and under 256 MiB on disk.
4. **An environment and usage.** `put_environment` with a variable and a
   binary file; a sandbox created with it sees `hi` and the file's exact
   bytes; `usage()` has the sandbox, running.
5. **Refusals.** A machine that is not there (404), a second of one name
   (409), a disk of 0 GiB (400), each raised as `ApiError` with the node's
   words; a resize to the size the disk has is no refusal.

## Result

All five passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-12,
with Python 3.13. [`report.json`](report.json) has the SDK's digest and each
refusal's words.

The first two runs stopped in case 4, in the check: it read the file through
`od`, whose spaced output the assertion did not expect, and then an edit to
the check meant to fix it did not apply. The node had written the right
bytes both times. The check now compares base64.

## What this does not show

- **Prompts.** `prompts.start` and `events` are tested against the fixture
  only; a prompt through a node is [checked with a stand-in agent](../../2026-10-11/machine-prompts-kvm/README.md),
  not through the SDK.
- **A control plane, keys and teams.** The node ran without keys.
- **Python older than 3.13** here; CI runs the fixture tests on its own.
