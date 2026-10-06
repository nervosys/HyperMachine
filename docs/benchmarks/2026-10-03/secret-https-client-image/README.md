# Separate owned HTTPS guest fixture

The exact accepted initramfs is augmented with installed host curl and its
33 shared-library/loader files. The base is immutable and verified by its fixed
SHA-256. No client is downloaded. Input hashes, curl version and image hash are
recorded in result.json. A second independent temporary build yields identical
image bytes. The image is 11,017,803 bytes; it is kept locally at
`/var/tmp/hm-secret-https-guest.cpio.gz`, separate from benchmark inputs.

The client starts under chroot and lists HTTPS support. This proves the client
and loader/library installation, not KVM networking or substitution. The daemon
already installs its interception CA in the guest default OpenSSL trust bundle.
Actual verified guest HTTPS requests, sandbox scope rotation, fork exclusion and
pause/resume remain to be exercised. This fixture adds dynamic libraries and
must not be used to claim latency improvements against the accepted image.

Reproduce as Linux root with installed trusted `/usr/bin/curl`, cpio and ldd:
`python3 tools/build-egress-client-image.py --base ACCEPTED.cpio.gz
--output NEW.cpio.gz --report NEW.json`. Outputs must not exist. The builder
accepts only the exact known fixture hash; extraction is not a generic untrusted
archive interface. Temporary filesystem trees are removed after each build.
