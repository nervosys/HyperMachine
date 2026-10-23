# A machine snapshot's archive through a control plane, on KVM (2026-10-23)

`tools/check-machines-cluster-kvm.py` (its new fifth case) on WSL2's KVM,
running as root, with an owned Redis, one node and `hv2-control-plane` over
verified TLS and node mTLS, and API keys for two teams. All 6 cases pass
(`report.json`); the new one:

Red stops its machine, snapshots it, and downloads
`/machines/web-01/snapshots/{id}/archive?path=/root` through the control
plane as a gzipped tar (`application/gzip`) holding `note` with the bytes the
guest wrote. Blue's request for the same snapshot is a 404. The snapshot is
deleted and the machine started again.

NOT VERIFIED: a partitioned disk or Ubuntu's cloud image, an archive over
64 MiB through the control plane, and a node that is not root.
