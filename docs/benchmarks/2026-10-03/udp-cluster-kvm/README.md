# Shipped CLI/control-plane/Redis/KVM UDP verification

Six checks pass through separately launched shipped CLI and control-plane development binaries, owned private Redis, the real sandbox daemon and an actual KVM guest UDP socket. Two local CLI peers receive exact isolated empty, four-byte binary and 65,507-byte payloads. Wrong control API keys receive 401. Raw authenticated UDP sessions also pass pause closure, resumed fresh-session payload and deletion closure checks. The control plane, Redis, daemon and CLI are reaped, guest inventory is empty, and owned snapshots are removed. Input hashes stay unchanged.

Reproduce with `python3 tools/check-udp-cluster-kvm.py --daemon /path/to/hv2-sandboxd --control-plane /path/to/hv2-control-plane --cli /path/to/hm --kernel /path/to/kernel --initrd /path/to/udp-echo-image.cpio.gz --output /fresh/output`. Redis must be installed. Fixture credentials are randomized and passed in environment variables, not command arguments; they are not archived.

Builds use the accepted isolated core and API source context. Source-context.json distinguishes current root and isolated sources; current root API TLS additions are not silently treated as tested by this older accepted API context. This is actual shipped-process HTTP routing with Redis, not an in-process router fixture.

The endpoint carries framed IPv4 UDP over HTTP/1.1 streams and CLI loopback sockets. It does not provide a native public UDP listener. TLS/mTLS for the combined path, IPv6, idle expiry, malformed frames, sustained load, loss behavior and UDP throughput/P99 remain unverified. No across-the-board feature or performance win is claimed.
