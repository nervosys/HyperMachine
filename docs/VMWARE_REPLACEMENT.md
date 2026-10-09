# HyperMachine as a VMware replacement on AWS GovCloud

**Goal:** run HyperMachine where an organisation runs vSphere (ESXi hosts managed by
vCenter): long-lived, general-purpose VMs, on a fleet of EC2 bare-metal hosts in AWS
GovCloud (US). It must meet FedRAMP High / DoD IL4–5 expectations for cryptography,
encryption at rest, audit and hardening.

This page is the plan and its status. Each pull request in the program updates its row.
An item is marked done only with evidence: a test, or a run on real KVM or a real host
that is archived under `docs/benchmarks/`.

## Where it starts (2026-10-07)

HyperMachine today is an ephemeral microVM **sandbox** platform. It is fast KVM boot,
fork and checkpoints, teams, SSO, egress policy and block disks, built for E2B-style
workloads. Against vSphere and GovCloud, the gaps found were:

- **VMs:** sandboxes have a 24 h lifetime cap and a RAM root filesystem, boot only a
  Linux kernel and initramfs, and do not survive a host restart.
- **Boot:**
  - there is no UEFI/BIOS firmware path; `hv2-core/src/uefi` is type models only;
  - there is no ISO install path;
  - disks are raw only (QCOW2 and VHDX are header parsers; there is no VMDK).
- **Operations:**
  - live migration is in-memory only, and KVM dirty logging is never called;
  - there is no HA restart, maintenance mode or resource-aware placement;
  - each VM has one disk and one NIC, with no hot-plug, behind a userspace NAT.
- **Government:**
  - TLS runs on `ring`, which is not a validated module, and nothing selects FIPS mode
    at runtime;
  - nothing is encrypted at rest, and there is no KMS integration;
  - the Terraform hardcodes `arn:aws:` and defaults to a public EKS endpoint;
  - the node runs privileged as root with no seccomp or jailer;
  - audit has no export or retention.

## Decisions

| Decision | Choice |
|---|---|
| Order | Phase 1 (persistent VMs) and Phase 2 (Gov baseline) are built in parallel |
| Guests | Linux first. Windows comes in Phase 4 |
| FIPS 140-3 | AWS-LC's validated FIPS module (through `aws-lc-rs`) for TLS and the primitives that need a validated module. IronCrypto stays for everything else until it has its own CMVP certificate |
| Deployment | A fleet of EC2 bare-metal hosts (a hardened AMI with systemd services), not EKS |

## Hosting on GovCloud

KVM needs hardware virtualisation, which on EC2 means a `.metal` instance. GovCloud
(US-East) offers several families that come in `.metal` sizes, among them M5n, C5n, M6i,
M7i, R8i, I4i and I7i. Each `.metal` size must still be confirmed per GovCloud region
before it is relied on. EC2 nested virtualisation on non-metal C8i/M8i/R8i instances was
announced in February 2026 for commercial regions only, so this plan does not depend
on it.

## Phases

Status is one of: not started, in progress, done (with evidence link).

### Phase 1: Persistent VMs

| Item | Status |
|---|---|
| A VM object separate from sandboxes: a definition, persistent state, no lifetime cap | **done**: [machines](MACHINES.md), [verified on real KVM](benchmarks/2026-10-07/machines-kvm/README.md) on a node and [through the control plane with two teams](benchmarks/2026-10-08/machines-cluster-kvm/README.md). The KVM run had one node; placement and lookup across several nodes are covered by a test against stand-in nodes. `hm sandbox vm machine` drives them from the CLI. No events for machines yet |
| Boot from a persistent root disk (raw), with root on `/dev/vda` | **done**: an ext4 root disk made from any template, [verified on real KVM](benchmarks/2026-10-07/machines-kvm/README.md) |
| Restart policy and autostart; VMs come back after a host or daemon restart | **done for daemon restarts**, [verified on real KVM](benchmarks/2026-10-07/machines-kvm/README.md). A host reboot is not yet tested |
| A network for a machine | **partial**: one NIC behind the node's egress gateway, with allow and deny rules kept with the machine, [verified on real KVM](benchmarks/2026-10-08/machine-network-kvm/README.md). Egress only: nothing can connect in, and bridged or VLAN networking is a Phase 3 item |
| QCOW2 read/write and thin images | not started |
| UEFI firmware boot (OVMF), so stock cloud images and ISO installers work | **partial**: the VMM [boots firmware by PVH](FIRMWARE_BOOT.md) with a disk on the PCI bus, and an unmodified CirrOS cloud image [reaches its login prompt on real KVM](benchmarks/2026-10-08/firmware-boot-kvm/README.md). A machine [made from a disk image](MACHINES.md#from-a-disk-image) boots this way, [verified on real KVM](benchmarks/2026-10-08/machine-firmware-kvm/README.md). No ACPI, one vCPU, no NIC, one firmware (Rust Hypervisor Firmware) and one image tried; ISO installers need edk2's CloudHv build, which has not been run |
| Serial console over the API, then a web console | **partial**: `GET /machines/{name}/console?tail=N` returns the serial output and `POST` types at it, which is enough to log in to a guest. It is request and reply, not a stream, and there is no web console |

### Phase 2: Gov baseline

| Item | Status |
|---|---|
| TLS through AWS-LC's FIPS module, with suites and groups restricted to approved ones | **done** in the [FIPS build](FIPS.md). The full suites of the six crates that carry the feature pass under it when run by hand (1,853 tests on Linux, 2026-10-08); CI runs only the provider tests under it. Distributing FIPS binaries waits on a licensing decision (`aws-lc-fips-sys` carries the OpenSSL license) |
| A `--fips` strict mode in every binary that refuses non-approved algorithms | **partial**: `--fips` on `hv2-control-plane` and `hv2-sandboxd` refuses a non-FIPS build. It covers TLS only; [non-TLS primitives](FIPS.md#what-it-does-not-cover-yet) still run outside a validated module |
| Encryption at rest: KMS envelope keys for snapshots, memory images, disks and volumes; encrypted EBS | not started |
| GovCloud infrastructure: partition-aware, private-only, IMDSv2, VPC endpoints, KMS; a hardened AMI | not started |
| VMM isolation: per-VM unprivileged process, seccomp, no privileged root | not started |
| Audit export to CloudWatch with retention, and node and guest actions covered | not started |
| Signed releases and images, provenance, SBOM published | not started |

### Phase 3: Operations

| Item | Status |
|---|---|
| Live migration: KVM dirty logging, a network transport, convergence | not started |
| HA restart on host failure; maintenance mode and evacuation | not started |
| Resource-aware placement (CPU, memory, affinity) | not started |
| Shared datastores (EBS, FSx or Ceph); several disks per VM; hot-plug over virtio-pci | not started |
| Several NICs per VM; bridged and VLAN networking; IPAM | not started |
| Disk snapshots and clones | not started |
| VMDK and OVA import | not started |

### Phase 4: Enterprise and Windows

| Item | Status |
|---|---|
| VGA and a VNC or web graphical console | not started |
| Windows guests and a Windows guest agent | not started |
| A vTPM device a guest can reach; Secure Boot in the firmware path | not started |
| PIV/CAC client-certificate sign-in; SAML; group-based roles | not started |
| A vCenter-style UI: inventory, lifecycle, console, alarms | not started |
