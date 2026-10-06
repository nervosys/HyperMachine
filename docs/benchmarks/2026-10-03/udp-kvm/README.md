# Authenticated real KVM UDP verification

Five checks pass through an owned standalone sandbox daemon, actual KVM guest, vsock transport and guest loopback UDP echo socket. Wrong configured cluster credentials receive 401. Empty, binary and maximum IPv4-size (65,507-byte) datagrams round-trip exactly. Pause closes an existing session, resume accepts a fresh session with exact binary bytes, and deletion closes it and empties guest inventory. All owned sockets are closed, the daemon is reaped and temporary snapshot files are removed. Input daemon/kernel/image hashes remain unchanged.

The guest image contains the previously verified static guest agent plus an authored static loopback-only echo fixture. The current image builder checks static ELF inputs and exact accepted base identity before fresh publication. The accepted base/kernel/release benchmark binaries remain unchanged. The report identifies the current development daemon and fixture image; it is not a release-performance result.

Reproduce with `python3 tools/check-udp-kvm.py --daemon /path/to/hv2-sandboxd --kernel /path/to/kernel --initrd /path/to/echo-fixture.cpio.gz --output /fresh/output`. Build the echo service with `gcc -static -O2 -Wall -Wextra tools/udp-guest-echo.c -o /fresh/echo` and include it using build-udp-guest-image.py --udp-echo /fresh/echo. Runtime inputs and test tool sources are archived.

This verifies authenticated node-level UDP payload and lifecycle behavior. It does not combine the separate CLI/control-router fixtures into an untested full-path claim. Real control-plane/CLI/TLS integration, concurrent peer isolation in KVM, malformed frames, idle expiration, IPv6 and throughput/P99 remain unverified. No competitor superiority is claimed.
