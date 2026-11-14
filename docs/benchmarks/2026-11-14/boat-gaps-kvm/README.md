# Four BOAT_PARITY claims, checked on KVM

Driver: `tools/check-boat-gaps-kvm.py`. Three runs, each in its own output
directory (`run1`, `run2`, `run3`; only `report.json` is kept, the daemon logs
are not). Every run succeeded: 4 cases and a "ran on KVM" check, all passing.
Each daemon logged `Detected hypervisor platform: Kvm` (2 or 3 times) and
never `Tcg`; the four daemons of a run are counted in `platform_lines`.

Host: WSL2 with nested KVM (`kvm_amd`). Release build of `hv2-sandboxd`, one
vCPU and 1024 MiB per sandbox, an Ubuntu initramfs with python3, a stock
kernel.

## Inputs (identical in the three runs)

| Input | SHA-256 |
| --- | --- |
| daemon | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| kernel | `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297` |
| initrd | `a22419caa282550c4bcd2b907a8aa9201c75fc8bb9859a1709ce023f6873174e` |
| driver | `251dc47fd6a92fdf30d70bba7c5a5c417e9fa2a778a3ad1fe75588834e48bc45` |

## Results

| Case | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- |
| 1. `--capacity 1`: a second create answers 503 | pass, 2.5 ms | pass, 3.8 ms | pass, 2.5 ms |
| 2. `envVars`: an exec sees them; another sandbox lacks them | pass | pass | pass |
| 3. `PUT /sandboxes/{id}/network` on a running VM, both verdicts logged | pass | pass | pass |
| 4. Terminal and standard input (Connect) | pass | pass | pass |
| ran on KVM | pass | pass | pass |

1. The first sandbox answered an exec; the second `POST /sandboxes` got
   `503 {"code":503,"message":"this node is full: 1 running and 0 starting, of 1"}`
   in the time above (client side, loopback). The list still held one sandbox.
   After the first was deleted, a create succeeded, so the refusal was about
   capacity.
2. A sandbox created with `envVars` `{BOAT_SECRET, BOAT_OTHER}` printed both
   from `exec` (also through `env` and a child `sh -c`); a second sandbox
   created without them printed empty values. The create reply and
   `GET /sandboxes`, `/sandboxes/{id}`, `/v2/sandboxes` did not contain the
   secret's value.
3. The daemon ran `--network --egress-default deny --tenant-reserved-cidr
   <this host's private address>/32`; the target was a TCP listener on that
   address that sends a token. Sandbox created with `allowOut` for it:
   the guest got the token. `PUT .../network` with `{}`: the next connection
   failed (`Connection refused`) and the listener saw no connection. `PUT`
   with `allowOut` for it again: the token again. The guest's `boot_id` was
   the same at the end (no restart). `GET .../network/decisions` held, for
   that target and in order: `allow (allowOut address)`, `deny (reserved
   address)`, `allow (allowOut address)`, and a fourth `allow` for a PUT that
   named the address in both `allowOut` and `denyOut`. That fourth step is
   recorded, not asserted: the address was reached, so `allowOut` won over
   `denyOut` for the same address. That is the intended precedence (E2B's
   reading, and what `tools/urltrace.py` relies on when it sends
   `allowOut: [host]` with `denyOut: [0.0.0.0/0, ::/0]`).
4. Over the sandbox's own `process.Process` listener (`processPort`, with
   `X-Access-Token`), spoken as the Connect protocol (`application/connect+json`
   for `Start`, JSON for `SendInput` and `CloseStdin`): a `/bin/sh -i` started
   with a pty of 100x30 reported `/dev/pts/0`, echoed the typed line, ran
   `echo pty-$((6*7))` (printed `pty-42`), reported `30 100` from `stty size`,
   delivered everything as `pty` events (none as `stdout`), and `exit 7` ended
   it with exit code 7. A process started without a pty (`cat; echo cat-done`)
   read two lines sent with `SendInput` `stdin`, and `CloseStdin` ended it:
   output `line one\nline two\ncat-done\n`, exit 0.

## NOT VERIFIED

- Case 1 is one refusal per run on an idle node, over loopback. It says the
  answer is immediate (milliseconds), not how it behaves under a burst of
  creates, with `--evict-idle-after` (which parks creates instead), or through
  a control plane. "There is no request option that says so" was not tested.
- Case 2 covers the E2B `envVars` field on the create call. Not shown: that the
  variables reach commands started over the Connect/envd listener or SSH, that
  they survive pause/resume, snapshot restore or a fork, the account-wide
  secrets that BOAT_PARITY compares them with, and values with unusual
  characters or large sizes.
- Case 3 reached one local TCP target by IP. Not shown: name-based rules, DNS,
  TLS, the Internet, a denial that comes from a `denyOut` list rather than
  from the default policy (the `deny` in the log has the reason `reserved
  address`, since the target is on a reserved private range the operator
  opened only by the sandbox's allow list; a `denyOut`-caused deny was not
  produced, and the same-address case went to `allowOut`), a change to a
  sandbox that is paused, and a list change racing a connection in flight.
- Case 4 used a hand-written Connect client in the driver, not the E2B SDK
  and not gRPC. Not shown: window resize (`Update`), signals, `StreamInput`,
  reattaching with `Connect`, binary or non-UTF-8 input (the daemon rejects
  it by design), a terminal through the control plane or SSH, and
  interactive programs beyond a shell.
- Nested KVM on a WSL2 host, a release build, a small initramfs guest: nothing
  here is a performance number, and a 2.5 ms refusal on this host is not a
  claim for another.
