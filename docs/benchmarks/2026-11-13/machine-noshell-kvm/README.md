# Machines on an image with no shell, on KVM

`tools/check-machine-noshell-kvm.py` boots a machine (`POST /machines`) on a node whose initrd is
the no-shell image from `../../2026-11-12/urltrace-noshell-kvm` (the guest agent is PID 1; no
`/bin/sh`, no busybox), with the node's `--network` on. Three consecutive runs, `run1` to `run3`,
each pass all cases. Each run directory holds `report.json` and `daemon-log.txt`.

This closes the part `../../2026-11-12/urltrace-noshell-kvm/README.md` listed as NOT VERIFIED
for the machine routes: the machine-boot network setup's no-shell fallback and the 400 on the
shell-only routes. The desktop-ready marker read is still not verified (see the end).

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V). Each run's daemon log
  has 3 lines `Detected hypervisor platform: Kvm` and 0 saying `Tcg` (counted by the driver and
  recorded as `platform_lines` in `report.json`).
- Daemon: Linux release `hv2-sandboxd`, sha256
  `26951a7e8337af6ac980e435e383e957454ed4c57010e47056c68752504579f6` (repo commit `969bf606`).
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Image `urltrace.cpio.gz`, sha256
  `e1e285cfdbddb5a0f318ebdbf3ef4540da51ead79a2bc327fe9042afd6860ec7`.
- Driver `tools/check-machine-noshell-kvm.py` hash is in each `report.json` (`input_sha256`).
- Daemon flags: `--no-template --network --machine-dir <tmp> --memory-mb 512 --cpu-cores 1
  --capacity 4`, `HV2_KERNEL`/`HV2_INITRD` set to the kernel and image.

## Results (identical in all three runs)

| Case | Result |
| --- | --- |
| a. machine with `network: {allowInternetAccess: false}` created from the no-shell image | 201, state `running`; `configure_network` did not fail |
| b. `GET /machines/{name}/files?path=/etc/ssl/certs/ca-certificates.crt` | 1 certificate, 619 bytes (the image has no bundle; the agent's file writes created it) |
| c. `POST /machines/{name}/exec {"cmd":"true"}` | 400, message `this guest's image has no /bin/sh, and this route runs its command through one (... could not start /bin/sh: No such file or directory (os error 2))` |
| d. `POST /machines/{name}/runs {"cmd":"true"}` | 400, same text with the agent's own wording of the error |
| e. argv exec of `/bin/hv2-urltrace` | **not supported**: the machine API takes only `cmd` (run through `/bin/sh -c`); argv exists only on `/sandboxes/{id}/exec`, covered by `check-urltrace-kvm.py` |
| f. `stop` then `start`, then `restart` | each answers 200, the guest's agent answers again, the bundle still holds 1 certificate after each; exec still 400 |

The check's pass lines, per run: six `ok:`/`not supported:` lines, `success: true` in
`report.json`. Case e is recorded with `"success": null, "supported": false`.

## Regression run on the normal image

Same daemon, kernel `bzImage`, tools image `tools.cpio.gz`:

- `tools/check-machine-network-kvm.py` (`../machine-network-kvm`): 5 of 5 cases ok, 5 `Kvm` lines,
  0 `Tcg`.
- `tools/check-machine-runs-kvm.py` (`../machine-runs-kvm`): 8 of 8 cases ok, 2 `Kvm` lines,
  0 `Tcg`.
- The network check's CA-once assertion counted every certificate in the bundle and expected 1,
  which held only for an image with no bundle of its own; the current tools image ships one, so
  it read 121. The daemon was right (the CA was appended once); the check now counts how often the
  bundle's last certificate (the appended one) is held, read through the files API, and expects 1
  after each of the three boots. It also decodes that certificate and requires the node's CA name
  (`HyperMachine sandbox egress CA`): without that, a node that appended nothing would pass, since
  the image's own last entry is held once too. The no-shell check makes the same name check on
  every certificate it counts. The network result and the three no-shell runs above are from a
  rerun with these assertions; all passed.

## NOT VERIFIED

- `tools/check-machine-desktop-kvm.py` was not run: it needs `--firmware`, a desktop-capable
  distro `--image` and a `--guest-agent` binary, none of which are present in WSL, and building
  them was out of scope. The desktop-ready check that now reads `seed::DESKTOP_READY` through
  `read_file_in_guest` is covered by unit tests only.
- The argv form on machines (case e) does not exist in the API.
- Only the first configure_network path on a no-shell guest is observed (bundle absent). A
  no-shell guest whose bundle already holds the CA, or a no-shell guest with a different bundle,
  is covered by unit tests of `bundle_with_ca` only.
- No timing claim. One vCPU, 512 MiB, nested virtualization.
