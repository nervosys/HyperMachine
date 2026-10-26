# A Docker volume's data across a checkpoint and a fork (2026-10-25)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, as root, the
Docker kernel (`build-kernel.sh --docker`) and the tools image with `--compose`
(the setup in [2026-10-23](../../2026-10-23/docker-sandbox-kvm/README.md)).
All 14 cases pass in 3 runs in a row (`runs/run1..3.json`; `report.json` is the
last). The new case is the 14th:

- A named volume `vdata` held `before`. A checkpoint was taken, the volume was
  changed to `after`, and the restore brought back `before`, read by a new
  container. A fork of the restored sandbox read `before` too.

NOT VERIFIED: a volume bigger than a few bytes; a volume on a bind mount or a
sandbox disk; RUN steps needing the network, a published port from outside
the node, `--userns-remap`, the default kernel (the 2026-10-23 list stands for
those).
