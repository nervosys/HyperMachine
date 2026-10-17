# A command's output, masked when it is read, on KVM (2026-10-16)

**What this checks:** the secrets a command *prints*, which the command-line
masking ([`COMMAND_HISTORY.md`](../../../COMMAND_HISTORY.md#secrets)) never
saw. Before this, "output is not scanned": a command that printed `env`
kept every secret in it, readable by anyone who could read the history.

## Method

`tools/check-sandbox-history-kvm.py` gained a case. In a sandbox on a real
guest it runs `printf` with an environment-style secret, a `name: value`
line, a `password: ...` line, an `Authorization: Bearer ...` header, a
GitHub token in a sentence, a PEM private key block and a prose line
("Basic usage: plain line"). It first checks the guest really printed them
(so the secrets are in the stream), then reads the command back through
`GET /sandboxes/{id}/commands/{n}` and checks that none of the secrets is in
the returned text, that each masked form is, and that `stdoutBytes` equals
the bytes printed and the stream's `next` equals `stdoutBytes`: the offsets
are the stream's own, not the masked text's.

## Result

All 9 cases of the check passed, 3 runs in a row (the 8 older ones are the
command history's: exec, the E2B SDK, command-line masking, history off, long
output, pause on one node, pause across two nodes through a store, and
refusals). What was read back:

```
API_TOKEN=[redacted]
name: ann
password: [redacted]
Authorization: Bearer [redacted]
key [redacted] here
[redacted private key]
Basic usage: plain line
```

[`report.json`](report.json), [`runs/`](runs/). Linux 6.18.33 under WSL2 with
nested KVM, the E2B SDK 2.51.0 for the SDK case. Unit tests cover the shapes
(`crates/hv2-sandboxd/src/history/output.rs`): an environment dump, JSON and
YAML, headers, prose left alone, URLs, key blocks whole and cut, nine kinds of
token and their near misses, spacing and non-ASCII. Clippy is clean.

## Not verified

- **Shapes beyond those listed**: a secret with no recognisable name or
  prefix is returned as printed, and so is one in a binary or compressed
  stream or split across lines.
- **A window that starts inside a key block** shows the body without the
  header; not exercised on KVM.
- **Over-masking in real logs**: any `key: value` or `*KEY*=...` line is
  masked; how often that hides something useful was not measured.
- **The raw stream in memory and in a paused sandbox's store record**: kept
  whole, as before; only what the API returns is masked.
- **`hm sandbox vm history` and the SDKs**: they print what the API returns,
  but were not run against this build.
- **One host, one kernel.**
