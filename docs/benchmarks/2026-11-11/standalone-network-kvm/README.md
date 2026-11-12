# Standalone-node VM-to-VM networking, on KVM

`tools/check-standalone-network-kvm.py`: one `hv2-sandboxd --network` node (no cluster, no
mTLS, no control plane); four sandboxes (A, B, C and a non-member), joined to a named network
through `POST /networks/{name}/connect` and then probed from inside the guests (resolve the
alias with `socket.gethostbyname`, open a TCP connection, read a known token back). The feature
is `crates/hv2-sandboxd/src/local_network.rs` and `networks.rs`, merged 2026-11-10
(`475f4d8a`); its scorecard line said "NOT VERIFIED against a live guest". Three runs, kept in
`run1` to `run3` (`report.json`, `daemon-log.txt`), found a defect; it was fixed the same day
and three more runs, `fixed-run1` to `fixed-run3`, pass every case. Both sets are below.

## Setup

- WSL2 Debian, kernel `6.18.33.2-microsoft-standard-WSL2`, `/dev/kvm` from `kvm_amd`, KVM under
  Hyper-V (nested). Each daemon log says `Detected hypervisor platform: Kvm`.
- Repo commit `60609f12` (HEAD) plus the uncommitted check script. Script sha256
  `3441be79...` (full value in each `report.json`).
- Daemon: the Linux release build `hv2-sandboxd`, built 2026-11-11 09:37 -0800, sha256
  `485e5153d259d7fb36e41ac035cc1b244477f49298d9a9026c7a29e2b436fc21`. Flags: `--network
  --memory-mb 2048 --cpu-cores 2 --capacity 4` (the default `--egress-default`, deny).
- Kernel `bzImage` (Docker + KVM options, 6.6.52), sha256 `07a76776...`; initrd
  `tools.cpio.gz` (Ubuntu 24.04 tools template, python3), sha256 `a22419ca...`.
- **The daemon was built before today's later commits.** It was built 21 hours after the
  feature merged (2026-11-10 10:50), so it contains the feature unchanged
  (`git log -- crates/hv2-sandboxd/src/local_network.rs` shows only `475f4d8a`). Two commits
  landed after the build and touch other crates: `b9820fe3` (hv2-agent examples, hm-cli) and
  `ec1a9de5`, which edits `hv2-core/src/devices/virtio.rs`, `devices/net_backend.rs` and
  `hv2-net/src/tap.rs`. I did not establish that those changes are irrelevant to the guest's
  virtio-net path, so the result is for the binary as built, not for `60609f12`.

## Result before the fix (`run1` to `run3`)

| Case | run1 | run2 | run3 |
|---|---|---|---|
| 1 create; in list and inspect | pass | pass | pass |
| 2 A and B join; inspect shows both with addresses | pass | pass | pass |
| 3 A resolves B's alias, TCP token round trip | pass | pass | pass |
| 3b third member C: A to C, C to A, C to B | pass | pass | pass |
| 4 reverse, B to A | pass | pass | pass |
| 5 non-member cannot resolve or reach by name or address | pass | pass | pass |
| 6 disconnect B: A's lookup and connect fail, A to C still works | pass | pass | pass |
| 7 reconnect B: works again | **FAIL** | **FAIL** | **FAIL** |
| 8 remove refused with members, deleted sandbox leaves, remove when empty | pass | pass | pass |

Three sandboxes were on the network (the original request's three VMs, with a mid-test
disconnect and reconnect). Resolution went through the guest's own resolver to the gateway
(`nameserver 10.0.2.3` from `/proc/net/pnp`), and the TCP connection to the private address
was carried to the peer; the data path, DNS scoping, the disconnect taking hold on the very
next lookup with nothing restarted, and the membership checks all work.

## The failure: a reconnected member is given an address another member holds

Case 7, all three runs, identical:

```
reconnected B was given 10.200.0.4, already held by a current member (a=10.200.0.2, c=10.200.0.4)
```

`after_reconnect_members` in each `report.json` shows `b.test` and `c.test` both at
`10.200.0.4`, as `GET /networks/{name}` returns them. Cause, read from the code:
`Network::next_address` (`local_network.rs`) picks `by_alias.len() + 2`. With A (.2), B (.3)
and C (.4) joined and B disconnected, two members remain, so the next join gets `.4`, which C
holds. `connect` does not check that the address is free. `owner_of` then returns whichever of
the two the `HashMap` yields first, so a connection to B's alias can be carried to C. The
discarded tries below saw both outcomes (a connect to B's alias answered with `TOKEN-C` in 3 of
7, `TOKEN-B` in 4), i.e. it is nondeterministic. This is a defect in the feature, not in the
check or the environment: two members of one network must not share an address, and the
unit test `reconnect_after_disconnect_restores_resolution_with_a_fresh_address` only has two
members at the time, where `len + 2` happens to be free. Any disconnect of a member other than
the last one joined, followed by a join, triggers it.

The check asserts address uniqueness before probing the reconnected member, so case 7 stops
there: **"B is reachable again after a reconnect" is not shown by these runs**, only that the
alias re-resolves (the lookup itself succeeded in the discarded tries). The feature's code was
not edited.

## After the fix (`fixed-run1` to `fixed-run3`)

`Network::next_address` now hands out the lowest address no current member holds, and a unit
test (`a_member_rejoining_is_never_given_an_address_still_in_use`) covers three members with
the middle one leaving and rejoining. The daemon was rebuilt (Linux release, sha256
`049b0357ff0080e5dd18783ab5e1bbdea410e06092fcf4123f55c169bd708406`, from the working tree at
`60609f12` plus that fix and uncommitted lint-only edits in `hv2-sandbox`), so these runs are
of current code, not of the morning's binary. Same kernel, initrd, flags and script.

| Case | fixed-run1 | fixed-run2 | fixed-run3 |
|---|---|---|---|
| 1 create; in list and inspect | pass | pass | pass |
| 2 A and B join; inspect shows both with addresses | pass | pass | pass |
| 3 A resolves B's alias, TCP token round trip | pass | pass | pass |
| 3b third member C: A to C, C to A, C to B | pass | pass | pass |
| 4 reverse, B to A | pass | pass | pass |
| 5 non-member cannot resolve or reach by name or address | pass | pass | pass |
| 6 disconnect B: A's lookup and connect fail, A to C still works | pass | pass | pass |
| 7 reconnect B: works again both ways, nothing restarted | pass | pass | pass |
| 8 remove refused with members, deleted sandbox leaves, remove when empty | pass | pass | pass |

Each daemon log says `Detected hypervisor platform: Kvm`. Case 7 now shows what the earlier
runs could not: the reconnected member has an address of its own and is reachable in both
directions.

## Earlier tries, discarded

`discarded-check-defect/try0` to `try6`: seven runs of earlier versions of the script. They
had a defect in the check (an empty-string shell argument shifted the probe's arguments, so
cases 5 and 6 never ran their probe), and in three of the seven the case 7 probe asked A for
B's alias and got `TOKEN-C` (the same address collision, before the check looked for it); in
the other four it got `TOKEN-B`, because the collision resolves by `HashMap` order. They
are kept because they are runs, but their case 5 and 6 results are void; the reported runs are
`run1` to `run3` with the fixed script.

## Not verified

- UDP, IPv6, any protocol but TCP; throughput or latency (no timing claim is made).
- More than one node, or this path combined with the cluster's private networks.
- The `hm sandbox vm network` CLI itself (this drives the HTTP API), and `compose` using this
  path.
- A WAN, a sandbox on several local networks at once, and more than three members.
- Egress policy interaction beyond the default (`--egress-default deny` worked; `allow` was
  not tried), and a sandbox forked or restored from a pause while a member.
- A build of exactly one commit: the passing runs are of a working tree, as noted above.
- Hosts other than this Hyper-V-nested AMD one.
