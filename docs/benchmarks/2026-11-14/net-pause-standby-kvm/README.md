# Pausing a networked sandbox, and standby, after the device hold (23d805bd), on KVM

23d805bd made `VM::pause` hold host-to-guest delivery (vsock and network) so a snapshot cannot tear, with
`resume` delivering what was held, and left `standby` not holding, since traffic is what ends it. Its own evidence
(`../../2026-11-13/restore-loop-kvm`) used vsock only. This directory covers the two things it left NOT VERIFIED:
standby on KVM, and the hold with a network device attached.

## Setup

- WSL2 (AMD), `/dev/kvm` via `kvm_amd`, run as root. Daemon release build of 23d805bd, sha256
  `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70`; kernel `07a76776...`.
- Images: the old-agent initrd `2952bbbe...` (standby, as in its 2026-10-08 evidence) and the tools image
  `a22419caa282550c4bcd2b907a8aa9201c75fc8bb9859a1709ce023f6873174e` (Ubuntu with python3, for the network runs).
- Each `report.json` carries its inputs' sha256 and the driver's.

## Results

| Dir | Driver | Cases | Result |
| --- | --- | --- | --- |
| `net-pause-tools` | `tools/check-net-pause-kvm.py --cycles 6` (new) | 20 | **pass**; `platform_lines` Kvm 15, Tcg 0 |
| `standby-old-image` | `tools/check-standby-kvm.py` | 6 | **pass** |
| `standalone-network-tools` | `tools/check-standalone-network-kvm.py` | 9 | **pass** |

- **Networked hold** (`check-net-pause-kvm.py`): sandboxes A and B on one network; A serves a token on TCP 9000, B
  connects every 0.25 s. Six cycles of pause/resume (2 s paused), snapshot, and fork of A, with B sending
  throughout. After every step A answered an exec and B reached A again within one poll (0.51-0.61 s). B's tries:
  52 ok, 61 failed (those during pauses and snapshots time out), **0 corrupted**.
- **Standby**: a standby guest makes no vCPU exits while left alone; the next command, and a request on a proxied
  port, wake it; pause-to-disk from standby resumes; an idle sandbox enters standby by itself and wakes the same
  way. So standby still wakes on traffic with delivery not held.
- **Standalone network**: TCP between guests through the gateway, with nothing held, still works.

The standby and standalone runs, and an earlier version of the network driver (19 of 19), were run by a subagent;
the `net-pause-tools` run here is of the committed driver, run again centrally. All daemon logs had 0 WARN, 0 ERROR
and 0 panic lines.

## NOT VERIFIED

- On KVM, that held network frames are delivered rather than dropped: TCP retries a lost SYN, so a dropped frame
  would also pass this check. The direct evidence is at unit level instead: `vm::tests::
  a_paused_vm_holds_host_frames_and_resume_delivers_them` (virtio-net over MMIO) and
  `a_paused_vm_holds_host_frames_on_pci_and_resume_delivers_them` (PCI) queue a frame through the real delivery
  thread while held and assert no used-ring write, the queue position unchanged and the frame pending, then after
  `resume` the frame's bytes in the guest buffer after the virtio-net header. With the gate disabled both fail
  ("a held device wrote the used ring"); with it, both pass. Run on Windows; CI runs them on all three platforms.
- Network reachability of a fork child (only its exec is checked).
- Which virtio-net transport (MMIO or PCI) the tools image's guest used in the KVM run is not recorded.
- `check-secret-substitution-kvm.py` (pauses and forks a networked sandbox over HTTPS): its client image
  `/var/tmp/hm-secret-https-guest.cpio.gz` no longer exists, and it was not rebuilt.
- Any host other than this AMD WSL2 one.
