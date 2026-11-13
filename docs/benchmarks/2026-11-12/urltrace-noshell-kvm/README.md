# URL tracing with no shell in the image, on KVM (shell-form exec answers 400)

The same check, image and node flags as `../../2026-11-11/urltrace-noshell-kvm/README.md`, rerun
after the daemon changed what a shell-form exec gets on a guest with no shell. Three consecutive
runs, `run1` to `run3`, pass all 17 cases each (cases 1-14 with 5a-5d; case 14 is the decision-log
flood from `../../2026-11-11/urltrace-decision-log-kvm`). Each run directory holds `report.json`,
`daemon-log.txt`, `image-files.txt` and `traces/`.

## What changed in the daemon

- `POST /sandboxes/{id}/exec` with `cmd` (run through `/bin/sh -c`) on a guest with no `/bin/sh`
  answers **400**, `this guest has no /bin/sh; send argv with an absolute program path instead of
  cmd (<the agent's error>)`, instead of a 500. An `argv` that names `/bin/sh` or `/bin/busybox`
  itself still gets the plain 500 with the agent's error and no advice to use argv.
- Case 11 now asserts both: the `cmd` form is a 400 that mentions `argv`, and neither `argv` form
  is told to use argv. `run1/report.json` case 11 holds the three answers as received.
- Not exercised by this check: the same 400 on `POST /machines/{name}/exec` and
  `POST /machines/{name}/runs`, the machine-boot network setup's no-shell fallback
  (`machines.rs` `configure_network`), and the desktop-ready check reading its marker file. Those
  are covered by unit tests and a Linux build only, **NOT VERIFIED** on a guest.

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V). Each daemon log has 20
  lines `Detected hypervisor platform: Kvm` and none says `Tcg` (counted with `grep -c`).
- Repo commit `ff9c3373` plus the uncommitted changes this run was made for
  (`crates/hv2-sandboxd/src/{guest_setup.rs,main.rs,machines.rs,machines/runs.rs,machines/desktop.rs}`,
  `tools/check-urltrace-kvm.py`). A one-line change to `crates/hv2-core/src/crypto/fips.rs` (a
  rustc 1.100 lint) was made while the build ran; whether this daemon includes it is not recorded.
- Daemon: Linux release `hv2-sandboxd`, sha256
  `26951a7e8337af6ac980e435e383e957454ed4c57010e47056c68752504579f6`.
- Kernel, guest image (`urltrace.cpio.gz`, sha256 `e1e285cf...`) and tracer: unchanged from
  `../../2026-11-11/urltrace-noshell-kvm`; their hashes are in each `report.json`
  (`input_sha256`).
- Before the runs: `cargo clippy -p hv2-sandboxd --all-targets -D warnings` exit 0 and
  `cargo test -p hv2-sandboxd` 193 passed, 0 failed, 2 ignored (Linux, rustc 1.99.1);
  `cargo fmt --check` and clippy exit 0 on Windows (rustc 1.98.1).
