# Host restore memory stages

Eight real single-guest restores pass with cleanup verified. Each of four measured HyperMachine guests has exactly ordered address-bound probes for mapping, machine, vCPU, device, pre-run, restored acknowledgment and command completion. Twenty-two damaged contracts are rejected with assertions disabled. Strict core/agent-library Clippy passes and Windows/Linux source regeneration is byte-identical. No runtime optimization is adopted.

| Phase | Median RSS KiB | Median anonymous KiB | Median private dirty KiB |
|---|---:|---:|---:|
| after_map | 0.0 | 0.0 | 0.0 |
| after_machine | 1412.0 | 1412.0 | 1412.0 |
| after_vcpu | 1412.0 | 1412.0 | 1412.0 |
| after_devices | 1412.0 | 1412.0 | 1412.0 |
| before_run | 1412.0 | 1412.0 | 1412.0 |
| after_notice | 21860.0 | 1440.0 | 21574.0 |
| after_exec | 27784.0 | 1848.0 | 26878.0 |

The host-stage observations precede first vCPU execution. Anonymous residency distinguishes anonymous pages from dirty file-cache classification, but a counter increase alone does not isolate the individual write operation or prove it redundant. Later observations permit guest/device activity. Probes and logging perturb timing, so all measurements are excluded from rankings. Single-guest findings do not establish the cause of the C100 gap. No equivalent Firecracker host-stage observations were collected. The frozen source, generator, binary SHA and activation-runner SHA bind the diagnosis to the actual instrumentation.

The map-to-machine interval also includes application of layered memory pages.
Named snapshot capture calls `checkpoint_to`, which uses `snapshot_layered`;
restore maps its base and writes the recorded changed pages before machine-state
restoration. Thus these observations do not attribute the increase solely to
KVM machine-state ioctls. A full sparse named-source image is a candidate for
avoiding per-child overlay writes, but requires separate capture, lifecycle,
state-preservation and matched performance verification before adoption.
