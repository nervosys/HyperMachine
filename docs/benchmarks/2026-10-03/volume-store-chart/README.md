# Independent persistent volume store in the sandbox chart

All ten chart tests pass and Helm lint reports no failures using the same
checksum-verified Helm tool identified in tool.json. The new node.volumeStore
claim configures --volume-dir and a writable /var/lib/hv2-volumes mount. It is
independent of the existing /var/lib/hv2-store snapshot claim. The four combinations
(neither, each alone, both) preserve coherent claim, mount and argument wiring.
Non-string claim names are rejected by schema. Prior operator egress and API
security chart tests continue to pass.

The chart references an existing claim, intended to be ReadWriteMany across
eligible sandbox nodes. It does not provision storage or verify a supplied PVC's
access mode/filesystem. Empty volumeStore preserves the daemon's snapshot-store
volumes fallback or transient storage. A dedicated claim persists volume data,
not guest memory or templates, and setting it does not migrate old files.
The operator guide documents these distinctions and backup considerations.

These are render tests, not a Kubernetes deployment or NFS/managed-storage
validation. No live resources were changed, no runtime or benchmark binary was
rebuilt, and no competitor or performance win is established. The archived files
are the exact working-tree bytes tested by Helm.

Reproduce with `python3 tools/test-sandbox-chart.py --helm HELM_BINARY` and
`helm lint deploy/helm/hypermachine-sandbox`; PyYAML is required.
