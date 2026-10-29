# `hm sandbox vm run -i` (KVM)

`tools/check-hm-run-kvm.py` gained a case; an owned `hv2-sandboxd` with a guest kit, no Docker.
The check now has 8 cases. **3 of 4 runs passed all 8** (runs 1, 3 and 4); `report-run1..4.json`,
`run1..4.log` are kept.

## Failed run

Run 2 failed in an older case, not the new one: `run curlimages/curl id -u` got a 503 "guest never
became ready: no answer from a guest agent on vsock port 1024 within 15s" right after pulling. That
is the occasional cold-boot stall already on the open list (1 in 14 before), not stdin. The stdin
case had passed in that run before it got there. Its log has the home path redacted.

## What the case checks

- `run -i alpine:3.20 cat` with input holding quotes, `$dollars`, a non-ASCII character and several
  lines gets exactly that text back.
- Empty input gives empty output.
- 300,000 bytes of input is refused with "longer than ..." and nothing is sent.

## Not verified

- A binary stream (the case uses text) and input just under the 256 KiB limit.
- Interactive use: input is read to EOF before the command starts; there is no `-t` or `-it`.
- Whether a very long command line is accepted by every node (the limit is the client's, 256 KiB
  before base64).
- `-i` with `--keep` (no case).
