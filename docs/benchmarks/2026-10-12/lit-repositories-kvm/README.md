# Lit repositories in an environment, in a sandbox on KVM

**What this checks:** an [environment's](../../../ENVIRONMENTS.md#repositories)
repositories, cloned with [Lit](https://github.com/nervosys/Lit) by the node
and put into a new sandbox. boat's environments clone repositories; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-lit-repositories-kvm.py` makes a Lit repository with `lit init`,
`add` and `commit`: `main` with `README.md` and `src/app.py`, and a branch
`feature` with one more file. It serves it with `lit server serve` on
loopback, with one admin account, and runs an owned `hv2-sandboxd` with
`--lit` (Lit 1.8.0, built from its tag) and that server as its only
`--lit-remote`.

1. **The environment.** `with-repos` holds the repository twice: on its
   default branch into `/srv/app`, on `feature` into `/srv/feature`, both
   with the account. The answer and the listing must name both and hold no
   password.
2. **In the sandbox.** A sandbox created with it must have `main`'s files in
   `/srv/app` and not `feature`'s, `.lit/` beside them, and `feature`'s file
   in `/srv/feature`. Every one must belong to the user its commands run as,
   and no archive may be left in `/tmp`.
3. **Refusals.** A remote on another port, which the operator did not allow
   (403); a wrong password (502, with the server's own words); a branch the
   remote does not have (502). Each must be answered before anything boots
   and leave no sandbox. Neither the node's log nor the Lit server's may
   hold the password.

## Result

All three passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-12.
From [`report.json`](report.json): the refusals read
`this node clones only from the Lit remotes its operator allows`,
`logging in as admin1: 401 Unauthorized Invalid credentials`, and
`the remote has no branch absent`.

The first run stopped in case 2, on the `feature` repository: Lit 1.8.0's
clone fetches every branch but makes a local branch only of the default, and
`lit checkout feature` does not find the others. The node now checks out the
fetched commit and makes the branch there. That, and the rest found while
building this, went to Lit's maintainers:

- only an `admin` account can clone from `lit server serve`; `reader`,
  `contributor` and `maintainer` get 403 on its transport routes;
- a failed clone says only `Operation failed` and leaves a half-made
  repository behind;
- session tokens expire, so a cloner stores a password and logs in each time;
- `lit status` reported a file staged after the `checkout -b`. Lit's
  maintainers found the cause is wider than checkout: `status` counts every
  index entry as staged, so no committed tree ever reports clean. Nothing
  here reads `lit status`;
- the README shows cloning from GitHub over HTTPS, which Lit's HTTPS
  transport, its own REST API, does not do.

## What this does not show

- **Any role but admin**, until Lit lets a reader clone.
- **TLS, a remote on another host, `lit://`.** Plain HTTP on loopback.
- **A template whose user is not root.** The base template's commands run as
  root, so "owned by its user" was root here.
- **A repository near the 64 MiB limit**, or a reboot putting it back.
- **A cluster**, where each node clones for the sandboxes it starts.
