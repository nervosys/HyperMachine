# Setup steps of a template, on KVM

**What this checks:** the claim in `docs/BOAT_PARITY.md` (row "A named set of variables, files and repositories
for new sandboxes") that setup steps run when a template is built. The only setup steps in the code are the
**steps of a template build** (E2B's `Template.build`): `RUN`, `COPY`, `ENV`, `WORKDIR` and `USER`, sent to
`POST /v2/templates/{id}/builds/{buildID}` (`crates/hv2-sandboxd/src/builds.rs`). `docs/ENVIRONMENTS.md` says
"a template runs its steps when it is built"; an environment itself has no setup-command field (its text:
"Setup commands run at creation" is about creating a sandbox, and no such API field exists in
`env_vars.rs`).

Driver: `tools/check-setup-steps-kvm.py` (new). It runs an owned `hv2-sandboxd` (`--guest-kit` an empty
directory, so nothing is pulled; `--memory-mb 512`, 1 vCPU) whose base template is the busybox image (it
needs `/bin/sh` for `RUN`), and builds templates `fromTemplate: base` through the API.

## Cases (all pass in `run1`, `run2` and `run3`)

| Case | What was shown |
| --- | --- |
| a. a `RUN` step ran at build | the step wrote the guest's `date +%s` to `/built-at`; in a sandbox created 4 s after the build the marker lay inside the host's window of the build (within 5 s), and 3 s or more before the sandbox's creation; the sandbox's clock was 4 s past it |
| b. a second sandbox does not run them again | `/runs`, a file each `RUN` run appends a line to, held one line in both sandboxes; the second sandbox's `/built-at` was the first's |
| c. order | one `RUN` after `COPY` (read the copied file), `ENV` (saw the variable) and `WORKDIR` (ran in it), then a second `RUN`: `/order` held `seeded`, `env=hello-env`, `cwd=/srv/setup`, `step-one`, `step-two` |
| d. a failing step | steps `RUN echo a`, `RUN ...; exit 7`, `RUN echo after-fail`: the build ended `error` with `step: "2"` and message `"echo before-fail > /b; exit 7" exited 7`; the build log stops at step 2 of 3; `POST /sandboxes` with that template answered 404 (`template "ss-fail" not found; this node has base`) and it is not in `GET /templates` |

Per run, `report.json` has `success: true`, four passing cases, `log_counts` of 5 `platform: Kvm` lines and 0
`platform: Tcg`, each build's log, and the sha256 of the daemon, kernel, initrd and driver
(`input_sha256`: daemon `672801943ae2...`, driver `42400b976a4f...`, the same in all three).

## Setup

- WSL2 (AMD), `/dev/kvm` via `kvm_amd`, run as root. Daemon: release build, sha256
  `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` (the build of 23d805bd, as
  `../../2026-11-14/net-pause-standby-kvm`). Kernel
  `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`. Image: busybox plus the agent,
  sha256 `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485`.
- 3 consecutive runs, `run1` to `run3`, each with `report.json` and `daemon-log.txt`.

## Observations worth knowing

- The built template is a **snapshot** ("Template ss-ok ready: snapshot of 2 MiB"): a sandbox from it is a
  restore, so it has the **same boot ID** the build's VM had (`build_boot_id_equals_sandbox_boot_id` is
  true in all three runs). The steps ran in a VM that was then snapshotted, not at the sandbox's boot.
- The node ran with no network for sandboxes ("this node gives sandboxes no network: steps run offline"), so a
  step that needs the Internet (`apt`, `pip`) was not tried.
- A `RUN` step is run through `/bin/sh`; with no shell in the image the build fails at that step with a message
  saying so (`../../2026-11-13/build-noshell-kvm/README.md`, case b).

## NOT VERIFIED

- A step that reaches the Internet, or a long-running one (the build's step timeout was not provoked).
- A build whose later step fails after an earlier one succeeded, where a *rebuild* of the same name might reuse
  the earlier steps' cache: the cache ("Steps kept as build-cache-...") was not exercised as a cause of a
  step not running.
- The control plane's template route (a team-scoped key is refused a build, per `BOAT_PARITY.md`): this ran
  against a node.
- `USER` and `COPY` steps are covered by `../../2026-11-13/build-noshell-kvm/README.md`, not here.
