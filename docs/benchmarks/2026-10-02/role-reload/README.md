# Live role reload verification

Two owned Linux control-plane processes each passed 33 HTTP checks. The initial
run and final hardened harness are both preserved; the final harness refuses
existing output files and retains explicit checks under `python -O`. Each run
used the previously verified clean observer-role binary with SHA-256
`d0757724cfcbacaeb80e64dade681a6299e7443f39bdda16bf458dd251bd00ef`.
No product code or binary changed for this verification.

The same scoped credential changed from operator/admin scope to observer/admin
scope through atomic file replacement and SIGHUP. Subsequent inventory requests
returned 200; volume access, create and GET tunnel requests returned 403.
Unknown and null roles produced explicit reload rejection acknowledgements and
kept the active observer restrictions. A later operator upgrade restored volume
inventory access. Observer/sandbox scope allowed sandbox inventory while
refusing template inventory. The legacy admin credential retained access.

The other checks cover revoked keys, expiry, malformed/empty/oversized/invalid
UTF-8 policies, missing policy files, and recovery after rejection. Each signal
waits for a new service acknowledgement before its HTTP checks. Both processes
remained live during the sequence and were stopped afterward. Binaries remained
unchanged and service logs omitted the random fixture credentials.

Run `python -O verify.py` here to check archive hashes, every expected HTTP
result, reload acknowledgements and owned-process cleanup.

This verifies Unix process-level policy replacement using an empty memory store;
it is not a tenant-isolation, performance or competitor comparison. Guest
mutation and capability checks against a real KVM guest are preserved in the
[observer-role archive](../observer-role/README.md). Existing authorized requests,
issued bearer tokens and open streams are unaffected by subsequent role changes.
