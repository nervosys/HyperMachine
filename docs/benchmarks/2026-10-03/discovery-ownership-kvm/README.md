# Pending discovery job: real ownership and port changes

Seventeen outer owned ACME checks and seventeen KVM checks pass. The worker
retains an actual issued-but-not-activated job after an owned activation fault.
A real unbind refuses removal of that pending job. A proof-authorized replacement
sandbox then claims the hostname, and a later guest-port update changes the
original binding. Both changes refuse job replacement with exact journal and
TLS manifest bytes, and the issued certificate hash, unchanged. Restoring the
original owner and guest port activates the retained certificate.

The existing 16 MiB TLS guest download completes and all subsequent routing,
reload, restart and guest lifecycle checks pass. Replacement guests are explicitly
tracked for cleanup. Owned processes are reaped and the node inventory is empty.
The report embeds the owned fault-wrapper source and hash. All tool sources and
accepted runtime inputs are bound to hashes; no production fault switch exists.

One excluded run expected HTTP 200 instead of the creation API's correct 201,
stopping before the temporary guest entered cleanup tracking. Its summary retains
the status error and nonempty-inventory cleanup failure; owned processes were
reaped. The raw failed response is omitted because it included a temporary guest
access token. The corrected fixture tracks replacement guests and status failures
no longer include response bodies. The failed run is not counted as a pass.

This verifies live disappearance, owner replacement and port changes while a job
is pending, under owned DNS proofs, ACME and KVM. Certificate retirement, public
DNS/CA behavior, fleet scale and atomic binding/TLS transactions remain incomplete.
No managed-product performance superiority is claimed.
