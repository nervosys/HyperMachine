# Sparse named-image lifecycle regression

The isolated candidate captures a full sparse image for named snapshots, rather than a memory layer, and adds image sidecar cleanup. It builds successfully and regenerates byte-identically on Windows/Linux from 550 accepted files. Strict daemon Clippy fails on the same two unchanged too-many-arguments functions in baseline and candidate; candidate lint passes with only that lint waived. No strict-lint clean claim is made.

A real KVM fixture preserves a prepared file, live process and independent child write. After source deletion, both baseline and candidate children still execute successfully and pause. The accepted child resumes and verifies the same state. The candidate fails resume with HTTP 500 because its paused layer refers to the deleted named-image .snap.mem file. Both owned nodes exit zero and guest inventory returns to zero; input hashes remain unchanged.

The candidate is rejected for adoption, and no performance comparison or win is claimed. The sparse-image timing driver now requires a passing lifecycle report with matching binary/image hashes before launching a timed cohort. Image lifetime must cover live children and persisted dependent snapshots before the optimization is evaluated. Replacement, restart, failure cleanup, shared-store races, physical storage cost and capture time remain required checks for any repair.
