# Compose services reach each other by name (KVM, one cluster node)

`tools/check-compose-network-cluster-kvm.py`: an owned Redis, one `hv2-sandboxd` node
(`--network`, guest kit, pulls `alpine:3.20` itself), `hv2-control-plane` over TLS and node
mTLS, `hv2-native-gateway`. The shipped `hm sandbox vm compose` is driven against the control
plane with a two-service file (`server`: `nc -ll -p 8080 -e /bin/cat`; `client`: `sleep`).
No Docker anywhere.

Six cases, **3 of 3 runs passed** with the script as committed (`report-run1..3.json`, `run1..3.log`):

1. `up` joins both sandboxes to the private network named after the project (`web`); the API shows the tag on each.
2. In `client`, `nslookup server.web.hv2.internal` answers with a `198.18.x.x` address.
3. In `client`, `echo ... | nc server 8080` (the bare name; the guest's `search web.hv2.internal` line expands it) is echoed back.
4. Another principal's project (`other`, blue key) cannot resolve `server.web.hv2.internal`.
5. `down` removes both sandboxes.
6. No API key or cluster token is in anything `hm` printed.

## What it took (failed runs are kept in the count)

Eight runs failed before the first pass (run 9). The failed logs were not kept; this list is
all that remains of them.

- Runs 1 to 4: `server.web.hv2.internal` was refused (`private network lookup`), while the
  name built from the sandbox ID resolved. My build line (`-p hm-cli --bin hm -p hv2-sandboxd`)
  did not rebuild `hv2-sandboxd` (`--bin hm` limited it), so all four ran the node built
  before the router alias. Run 5, with the node rebuilt, resolved the name.
- Runs 5 to 7: the TCP connection was refused (`private network refused`). The check's
  node certificate was `serverAuth` only, and the node presents it as a **client**
  certificate when it dials a peer node. Adding `clientAuth` fixed it. (The node certificate
  also needs `DNS:hv2-node`, the name the private router verifies peers against.)
- Run 8 (with `clientAuth`): cases 1 to 3 passed, then a check bug (blue's sandbox was
  exec'd with the red key, 403).
- Run 9 was the first full pass. Then 3 more with `ports:` (`with-ports-variant/`), one
  without it, and 3 of 3 with the committed script.

`with-ports-variant/`: the same check with `ports: ["9000:8080"]` on `server`, 3 of 3. I added
that while chasing the refusal above; one run without it also passed, and the committed script
has no `ports:`. So a sibling does **not** need the port published.

## Not verified

- One node only. Compose services on two nodes: NOT VERIFIED (the router and transport are
  checked across two nodes by the earlier private-network evidence, not through compose).
- UDP, a service name that equals another sandbox's ID, and a project name that is not a valid
  DNS label (it gets no network; unit-tested only).
- A compose file whose service names contain `_` (not valid DNS labels): NOT VERIFIED.
- A node without `--network`/cluster: `hm` treats a 404 from the membership route as "no
  private networks" and carries on; only the code path, not a real standalone node, was run.
- Restart, checkpoint or fork of a running project: the membership is not exercised there.
- Timing: none claimed.
