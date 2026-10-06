# Sandbox chart operator egress files

All eight sandbox chart tests pass with checksum-verified official Helm 4.3.0,
and Helm lint reports no failures. Tool source and hashes are in tool.json.
The tests cover defaults, each operator file alone and both together, selected
Secret key/path/argument wiring, networking and key/type refusal, and the prior
control-plane auth/TLS configurations.

The chart copies selected existing Secret keys at startup using an init container
with the node image's /bin/sh and cp. Projected input keys are read-only; copied
files live in an in-memory emptyDir. The daemon sees only that volume read-only,
with 0600 regular single-linked files under an owned 0700 directory. The local
test executes the rendered script against symlinked projection-like inputs and
verifies ownership, modes, link count and contents. Init capability/root-filesystem
settings are rendered and asserted; actual container enforcement is not exercised.

Both configurations require node networking. The init copy does not follow live
Secret updates, so changing either Secret requires node pod restart. This remains
node-local exact-ID policy, requiring consistent configuration on eligible nodes;
it is not a managed organization secret service. The operator guide documents
that behavior and upstream trust scope. No secret values are added to chart
values or archived manifests. Rendered auth credentials are not archived.

These are chart-render and local filesystem checks, not a Kubernetes deployment,
image build, managed competitor comparison or performance benchmark. The node
image must contain the new daemon options before use. No resources were deployed,
no benchmark binaries changed and no external publication occurred.

Reproduce: `python3 tools/test-sandbox-chart.py --helm HELM_BINARY`, and
`helm lint deploy/helm/hypermachine-sandbox`. PyYAML is required; the script-copy
test requires Linux. The archived source bytes are the files tested locally.
