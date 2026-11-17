# Principal-scoped template builds (O14), checked on KVM

The O14 change in [TEAMS.md](../../../TEAMS.md) ("A key with a principal and no team"): a non-admin key
with a principal and no team builds templates under `scope.<16 hex of the principal hash>--<name>` on the
node; the control plane adds the prefix, resolves the plain name for that key and refuses other principals.
Its unit tests (`crates/hv2-cluster/tests/principal_templates.rs`) run against a fake node; this run puts it
in front of a real `hv2-sandboxd`.

Linux under WSL2 with nested KVM, toolchain 1.100.0, release build (`-D warnings`) of HEAD `002a00c3` **plus
the uncommitted O14 change** (`control.rs`, `ownership.rs`, `TEAMS.md`, the new test file). sha256:

| Binary | sha256 |
|---|---|
| `hv2-sandboxd` | `a6ccec92e4a0db47df65b446aca5528ef48edb84029015bbd778a2b233d6c21f` |
| `hv2-control-plane` | `0eb86cdd44a096abb10a4df946ae82e85d46f6b5195b74c9b5b0eaa7c11ec602` |

Guest: kernel `/var/tmp/hm-docker/bzImage`, initrd `/var/tmp/hm-batch/busybox.cpio.gz`.

[`check-principal-templates-kvm.py`](check-principal-templates-kvm.py) starts an owned Redis, one node
(`--guest-kit` an empty directory, cluster token shared with the control plane) and one control plane
(plain HTTP on loopback, `--api-keys-file`) with three keys generated at run time: `alice` and `bob`
(principal, no team, scopes `sandboxes`, `inventory`, `templates`) and an administrator. It builds through
E2B's API (`POST /v3/templates`, `POST /v2/templates/{id}/builds/{buildID}` with one step
`RUN echo hi > /hello`, `fromTemplate: base`, then `GET .../status`). Results: [report.json](report.json)
(every call's status in `calls`); logs: `node.log.txt`, `control.log.txt`.

## Results (all cases passed)

| Case | What was done | Result |
|---|---|---|
| 1 | alice builds `app` | 202 on start; status reaches `ready`; the node log shows `template scope.2bd806c97f0e00af--app built by steps`, so the real node accepts the scoped name (the build answered with that `templateID`) |
| 2 | alice creates a sandbox with `templateID: app`, `cat /hello` | 201; prints `hi` |
| 3a | admin `GET /templates` | holds `scope.2bd806c97f0e00af--app` |
| 3b | bob, by plain name `app`, by canonical `scope.…--app`, and by alice's template ID: `GET /templates` | alice's template is not listed |
| 3b | the same three names: build status, `POST /sandboxes`, `DELETE /templates/{id}` | 404 in all 9 |
| 3c | bob builds his own `app` | `ready` under `scope.81b637d8fcd2c6da--app` (differs from alice's); bob's sandbox from `app` prints `hi`; bob does not see alice's |
| 4 | admin `GET /templates` | lists alice's template |
| 5 | alice `DELETE /templates/app` | 200; gone from alice's and the admin's lists; bob's `app` is still listed |

**Platform:** the node log holds 5 `platform: Kvm` lines and 0 `Tcg`.

## Notes

- The harness first failed twice for its own reasons, fixed in the driver: template routes need the
  `templates` scope on the key (403), and the node needs `--guest-kit` to build (501). Neither is O14.
- The driver also asks the node's `/templates` directly with the cluster token to list stored names; the
  node answered 401, so `node_templates` in case 1 says so. The scoped name on the node is shown by the node
  log and the admin listing instead.
- The Redis binary is an unpacked package (`/var/tmp/hm-keyroles/redis-pkg`); the driver takes it with
  `--redis` and `--redis-libs`.

## NOT VERIFIED here

- **Team keys:** a team key's 403 on builds was not run (unit tests cover it).
- **`COPY` uploads and `app:v1` tags:** only a `RUN` step and the default tag were built.
- **Name length:** a name at the 40-character limit was not tried on the real node.
- **Several nodes / TLS and mTLS:** one node over plain HTTP; the node-proxy path was not exercised.
- **Rebuild and overwrite:** a second build of `app` by alice, and bob naming `scope.…--app` on a build
  (403), were not run.
- **Repeats:** one run; no flakiness measured. No timing claimed.
