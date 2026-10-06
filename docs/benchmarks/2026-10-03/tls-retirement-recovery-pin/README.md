# Retirement recovery receipt isolation

All 18 owned local ACME/live TLS checks passed using the accepted control-plane
binary and unchanged deployment hook. After a hard exit following retirement
manifest publication, a wrong receipt was rejected while preserving exact
manifest and pending-journal bytes, the previous live leaf and generation-file
inventory. The correct receipt then recovered, verified fallback activation and
cleared the journal. Idempotent retry and reprovisioning also passed.

Owned processes and the HTTP thread shut down successfully. This extends the
receipt-bound retirement archive with a recovery-specific negative check; it
does not verify automatic worker retirement, public CA operation or KVM traffic.
