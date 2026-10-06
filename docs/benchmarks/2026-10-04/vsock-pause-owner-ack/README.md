# Acknowledged-pause release audit (failed)

The isolated release build passed, as did 47 selected core tests (2 ignored), 531 agent tests and 64 daemon tests (2 ignored). The candidate adds owner acknowledgements and reserved rollback commands to pause, alongside the experimental per-connection vsock wake change.

The planned 20-resume audit failed on the original main-target resume before any additional cycle. Pause and snapshot completed; restore and execution started, but the API timed out after its unchanged 30-second deadline. The restored vCPU exited with 6 exits during teardown. No final report, cycle journal or final guest-count proof exists. This does not establish a lifecycle fix or an accepted CPU improvement. The checker attempted cleanup; daemon/Redis shutdown is recorded, and process-inventory.json records a separate post-run observation.

The candidate SHA-256 is recorded in source-context.json. Source snapshots, regression logs, executed drivers, checker and runtime failure output are preserved. Protected root backend and boot files were excluded; the build used the accepted isolated core. The success-only verifier was not run. Production changes remain experimental and unstaged.
