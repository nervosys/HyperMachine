# Upload source type verification

Unix upload opens now use O_NONBLOCK before checking the opened descriptor's regular-file type and 4 GiB size limit. This prevents a named pipe without a writer from blocking both the integrated CLI and Python helper. Regular-file symlinks retain their existing behavior; the opened descriptor is checked rather than trusting a preliminary path check.

The shipped CLI regression creates an owned FIFO with no writer, requires failure within two seconds, checks the regular-file error, and verifies no listener connection. The Python fixture similarly checks prompt failure and zero HTTP requests. Five volume CLI tests, twelve existing VM CLI tests, and nine Python helper tests pass. An offline locked isolated CLI build also passes. Protected root core sources were not used for the build.

Reproduce with `cargo test --offline -p hm-cli --test volume_client --test sandbox_vm_client` in an accepted source checkout and `python3 tools/test-volume-upload-client.py`. Unix FIFO tests are skipped on other platforms. O_NONBLOCK does not establish a total deadline for filesystem/device operations; regular upload sources must remain stable during transfer. No maximum-size transfer, performance improvement, or competitor advantage is claimed.
