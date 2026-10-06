# Recovery after repeated cluster-store timeouts

Two owned KVM profiles passed 32 checks each: SET refusal with IPv4/two peers and XADD refusal with IPv6/eight peers. Both initial creation and resumed guests are preserved while Redis publication is refused. Redis WRITE commands are then paused, permission restored, and at least two store-timeout failures for each guest observed in daemon logs. Pending discovery still identifies exactly the preserved guest.

After CLIENT UNPAUSE, the worker recovers without a manual reconciliation request. Initial exact command output, unchanged resumed access tokens, stable native TCP/UDP payloads, and complete guest/process cleanup are verified. Input hashes match frozen development binaries. `timeout-events.json` retains the relevant timestamped failures without fixture credentials.

The store deadline expires before the worker's five-second outer deadline, so this proves repeated store-timeout recovery, not the outer timeout branch. Daemon-crash recovery and runtime fairness with more than 32 pending guests remain unverified. This is functional evidence and supplies no competing-product performance result.

Earlier attempts are excluded: the first incorrectly required the outer timeout log despite shorter store timeouts; the second stopped at an API-listener readiness race before the IPv6 recovery scenario. The fixture now waits for the actual listener after shared-template discovery, with a bounded wait and process-exit check.
