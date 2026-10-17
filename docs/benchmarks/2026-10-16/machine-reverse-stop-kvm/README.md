# A stopped machine closes the reverse connections waiting on it (2026-10-16)

**The defect:** a caller keeps reverse connections waiting on a machine's
port ([`REVERSE_PORTS.md`](../../../REVERSE_PORTS.md)); the node allows 64
for a machine at once. When the machine stopped, nothing closed them. They
stayed open, and stayed counted, until the node restarted: stop and start a
machine with its 64 waiting and the next connection was refused with 429.

**The fix:** stopping a machine, and the supervisor noticing its guest has
stopped by itself, now close every connection waiting on or carried from it
(`forwards::stop`, as a sandbox's pause and end already did).

## Method

`tools/check-machine-reverse-port-kvm.py` gained a fifth case. It opens 64
reverse connections for guest port 7002 (all accepted), checks the 65th is
refused (429), stops the machine through the API, and counts how many of the
64 end within 20 s. It starts the machine again and opens one more, which
must be accepted.

## Result

| | the 64 waiting connections after the stop | the next one after a restart |
|---|---|---|
| **Before the fix** (the release daemon built earlier the same day) | 0 of 64 ended | not reached; the case failed |
| **After the fix**, run 1, 2 and 3 | 64 of 64 ended | 101 (accepted) |

The first four cases of the check (through the API, through `hm`, the port
closing when the caller goes, the refusals) passed before and after, in
every run. The cluster check
[`check-machine-reverse-cluster-kvm.py`](../machine-reverse-cluster-kvm/README.md),
which also stops a machine, passed once more with the fixed daemon.
`report.json` and `runs/` hold the reports. Linux 6.18.33 under WSL2 with
nested KVM. `cargo clippy -p hv2-sandboxd --all-targets -- -D warnings` is
clean on Linux.

A first version of the case counted a socket timeout as "ended" and so could
not have failed; it was stopped and corrected before any result here.

## Not verified

- **A guest that stops by itself.** The supervisor path got the same call but
  was not exercised: the check stops the machine through the API.
- **A machine that is deleted** (it must be stopped first, so it passes
  through the stop), **restarted** (stop then start), or **a node that
  restarts**.
- **Connections already carried** (not waiting) at the stop: they close by the
  same call, but the check only had waiting ones.
- **Several machines at once**, and **a sandbox**, whose paths are unchanged.
- **One host, one kernel.**
