# Reproducible UDP KVM guest fixture image

Built the authored UDP guest agent statically from the accepted isolated source with `RUSTFLAGS="-C target-feature=+crt-static" cargo build --offline --locked --release --target x86_64-unknown-linux-gnu -p hv2-guest-agent --bin hv2-guest-agentd`, using a separate target directory. The fixture builder rejects an ELF interpreter, checks the exact accepted base image hash, replaces only /bin/hv2-guest-agentd, normalizes archive timestamps/ownership and publishes fresh output files. Input hashes are rechecked before publication.

Two independently packed images have identical SHA-256 f0a6e9bbab62caa3203e37deb757e94620bc34b783095b0eb9b35a569739b35e. The accepted base image and immutable daemon/kernel inputs were not overwritten. Reports identify the static agent and image bytes.

Reproduce with `python3 tools/build-udp-guest-image.py --base /path/to/accepted.cpio.gz --agent /path/to/static/hv2-guest-agentd --output /fresh/image.cpio.gz --report /fresh/report.json`, repeat with fresh output names and compare hashes. Linux cpio/readelf are required.

This establishes build reproducibility and the input fixture for subsequent KVM checks. The new image has not yet booted in a UDP KVM verification, and no payload, lifecycle, control-plane, TLS or competitor claim follows from image construction alone.
