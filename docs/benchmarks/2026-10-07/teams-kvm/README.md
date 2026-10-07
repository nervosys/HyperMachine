# Two teams on one real KVM cluster

**What this checks:** team isolation ([guide](../../../TEAMS.md)) end to end, through
the shipped control plane and node binaries, on real guests. It is the integration
evidence for the teams PRs: sandbox access, events and webhooks, volumes, and snapshots.

## Method

`tools/check-teams-kvm.py` runs an owned Redis, one `hv2-sandboxd` node and
`hv2-control-plane`. The API uses verified TLS and the control-to-node link uses
mTLS, each from an owned CA. The key policy file has two red keys (principals
`red-a` and `red-b`, team `red`) and one blue key (team `blue`), plus the legacy
administrator key. Every step goes through the control plane's public API.

1. **Sandboxes.** Red's second member lists red's first member's sandbox, reads it
   and runs a command in it. Blue gets 403 on GET, DELETE, exec, pause, fork,
   checkpoints and logs, and lists only its own. The administrator lists both.
2. **Volumes.** Red and blue each create a volume named `data` and get different
   IDs. A red guest writes a file through its `data` mount. A blue guest mounting
   `data` finds it empty. Blue reading red's volume by ID gets 404.
3. **Snapshots.** Red writes a marker and snapshots its sandbox as `red-snap`. Blue
   neither lists it in `/templates` or `/snapshots` nor can start from it or
   delete it (404 each). Red's other member starts a sandbox from it and reads the
   marker back. Red deletes it (204).
4. **Forks.** Red's second member forks red's first member's sandbox. The fork
   lists in red, and blue reading it gets 403.
5. **Events and webhooks.** Red creates a webhook. Blue lists no webhooks and gets
   404 for red's. Blue's event list holds events, none of them about red's
   sandbox, while red's holds that sandbox's.

## Result: all five pass

The full record is in [`report.json`](report.json). It includes the sandbox and
volume IDs: red and blue `data` were `vol-7173ff45a7747a83` and
`vol-bc4a76b18c6185e2`. Blue's event list had 2 events and red's had 4.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release, branch `feat/team-snapshots`) | `f12d8a1da3a16994fdb22eca0ba2474e1ff17263555db7477babc49c1397c077` |
| `hv2-control-plane` (same build) | `14765d54f1d5ce32d81c60903e0a238b52925bb09dbd1a6eaf05d89ad8909048` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-teams-kvm.py` | `d7d901ce18c7c2372fa1ea72fac60df680460026e3428ba9775e9f6406c218f7` |

The host was WSL2 kernel 6.18.33.2 with real KVM, Redis 8.0.2 and OpenSSL 3.5.7.

## Not shown here

- More than one node. Placement by team-owned template was checked against fixture
  nodes in the unit and integration tests, not across real nodes.
- Webhook delivery to a receiver. That is covered by
  `events::tests::a_team_webhook_receives_only_its_teams_events`, not here.
- Image template builds. Those remain administrator-only.
- SSO, durable team membership and per-team private networks. None of these exist yet.
