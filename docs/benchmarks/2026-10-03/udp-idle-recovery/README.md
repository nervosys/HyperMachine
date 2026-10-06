# Real UDP CLI idle slot recovery

Nine full-stack HTTPS/KVM checks pass. With max-peers=2, two CLI peers exchange exact payloads, then remain idle for 33 seconds. The same listener subsequently returns exact binary replies to an existing peer and a new third source address/port, verifying stale sessions release bounded peer slots. A fresh raw session is opened and byte-checked immediately before pause so that pause closure is not mistaken for the earlier idle expiry. Resume and deletion checks remain passing.

All owned CLI/control-plane/Redis/daemon processes are reaped, guest inventory is empty and input hashes remain unchanged. Reproduce check-udp-cluster-kvm.py with --tls --idle-check and the archived input paths. The wait tests recovery after the configured idle deadline, not an exact expiration timestamp or a sustained churn/load bound. Node traffic remains HTTP; mTLS, IPv6 and competitor performance are unverified.
