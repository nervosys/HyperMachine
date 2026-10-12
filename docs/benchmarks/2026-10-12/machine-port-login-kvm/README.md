# A login in front of a machine's port, through the control plane

**What this checks:** a machine's port [published with `proxy`](../../../MACHINES.md#published-ports),
reached through the **control plane's** proxy behind its
[operator-provisioned login](../../../PRIVATE_GUEST_URLS.md), as a sandbox's
ports are. boat puts a token in front of its ports unless they are made
public; see the [comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-port-login-kvm.py` runs Redis, a node with mTLS and a
machine directory, and a control plane with web access, proxy TLS and node
mTLS, as PRIVATE_GUEST_URLS.md requires, all with certificates from a CA it
makes. Through the control plane's API it creates a machine, starts BusyBox
`httpd` in it on ports 8080 and 9090, publishes 8080 with `proxy` and 9090
without. Every request below goes to the control plane's proxy over TLS, with
`8080-{machineID}.machine.test` as its host.

1. **Refused.** No credentials, the wrong password, and the right one while
   the policy's grant names no target: each 401 with a Basic challenge.
2. **Through.** The policy rewritten to name the machine's ID and reloaded
   with SIGHUP: the page comes back, `Cache-Control: private, no-store`; and
   still 401 with no credentials.
3. **Only what was published with `proxy`.** With credentials, 9090
   (published without `proxy`) and 22 (not published) are not reached.
4. **Not a machine's ID.** `vm-ZZZZ`, granted in the policy so the login lets
   it through, is refused by the route: no node is asked for it.

## Result

All four passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-12.
From [`report.json`](report.json): 401 for each refusal in case 1, 404 for
each port in case 3, and 404 for `vm-ZZZZ`.

Two runs failed first, both in the check. The node was started with a `PATH`
that had no `mkfs.ext4` in it, so no machine could be made. Then case 4 passed
for the wrong reason: the policy naming `vm-ZZZZ` was written and not
reloaded, so the login refused it before routing was tried. The check now
reloads, and requires a refusal that is not 401.

## What this does not show

- **That no node was asked** for `vm-ZZZZ`, beyond its answer: the shape is
  checked before any node is asked, and that is unit-tested
  (`only_a_machine_s_own_id_shape_is_asked_of_nodes`).
- **Single sign-on** in front of a machine's port; only the Basic login.
- **More than one node**, or a machine on a node the control plane cannot
  reach.
- **The guest seeing the caller's identity.** BusyBox `httpd` was not asked
  to print `X-HyperMachine-User`.
