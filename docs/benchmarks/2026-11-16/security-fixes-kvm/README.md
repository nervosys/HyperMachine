# Security fixes, re-checked on KVM

After the fixes in [SECURITY_AUDIT_2026-11-16.md](../../../security/SECURITY_AUDIT_2026-11-16.md),
three binaries were rebuilt from the fixed tree. The build used cargo 1.100.0 with
`RUSTFLAGS=-D warnings`:

| Binary | sha256 |
|---|---|
| `hv2-sandboxd` | `2056b506899966a3a84b158380014babc74981f5a264885a869befbc72d50a75` |
| `hv2-control-plane` | `2bd4145550e7710fffad77d165ac313b112d090ebe2d0684f157862a48ef2772` |
| `hm` | `3ab89b43431c5644617359b487376237f5f47b5bf61de6274bd0d442f4ca5ca9` |

Existing KVM checks that exercise the changed code were then run once each, on Linux under WSL2 with
nested KVM.

| Check | Exercises | Result |
|---|---|---|
| [`check-net-pause-kvm.py`](net-pause/report.json) | virtio-net and vsock under traffic, with ring validation, kick budgets and feature masking; pause, snapshot, fork and standby | pass. `platform: Kvm` 15 times, Tcg 0 |
| [`check-disk-pause-kvm.py`](disk-pause/report.json) | virtio-blk descriptor and status handling; a disk across pause and a new sandbox | pass. Kvm 8, Tcg 0 |
| [`check-hm-cli-kvm.py`](hm-cli/report.json) | the node API through the CLI (exec, files, checkpoints, fork, networks, disks) on the new loopback default bind | pass, 16 cases. Kvm 10, Tcg 0 |
| [`check-key-api-kvm.py`](key-api/report.json) | managed keys sealed with the new `--managed-key-secret-file`, two control planes over one Redis, revocation, expiry, and 503 with Redis down | pass, all cases. Kvm 2, Tcg 0 |
| [`check-machine-firmware-kvm.py`](machine-fw/report.json) | a stock Ubuntu 24.04 image booted by Rust Hypervisor Firmware through virtio-pci and virtio-blk | **partial.** The machine booted by firmware to `ubuntu login:` on KVM (Kvm 1, Tcg 0). The next case logs in as `cirros`, because the check was written for a CirrOS image that a WSL reset deleted. Ubuntu refused that login, so the console-input case did not run. This is a fixture mismatch, not a regression. |

## How the checks were run

- **net-pause:** busybox-plus-Python tools image (`a22419ca…`).
- **Other sandbox checks:** busybox-plus-agent initrd (`85d929ac…`).
- **Kernel:** `07a76776…`, for all of the above.
- **Firmware check:** firmware `4a0a1e97…`, image `ff40d989…`, the same as
  [desktop-browser-kvm](../../2026-11-15/desktop-browser-kvm/README.md). It also needed `HV2_KERNEL` and
  `HV2_INITRD` exported; a first attempt without them exited at start and is not counted.
- **Repeats:** one run each, not three. These are regression checks of existing behaviour after a
  change, not new claims.

## NOT VERIFIED here

- **Attack paths:** none were exercised on KVM. A hostile guest driver, a hostile template image and a
  hostile registry were not run. Those are covered by unit tests that fail on the old code, listed in
  the audit.
- **Startup refusals:** the new `--bind` and `--allow-unauthenticated` refusals were not exercised end to
  end; that is unit-tested only.
- **SSO:** guest URLs with the new `--guest-domain` were not run on KVM.
- **Machines from templates:** the code path where the S1 fix lives (`build_root`) was not run on KVM.
