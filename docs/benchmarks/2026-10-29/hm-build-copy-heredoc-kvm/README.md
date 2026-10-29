# `COPY <<EOF` in `hm sandbox vm build` (KVM)

`tools/check-hm-build-kvm.py` gained a tenth case; an owned `hv2-sandboxd` with a guest kit, no
Docker. **3 of 3 runs passed all 10 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The Dockerfile has:

- `COPY --chmod=755 <<EOF /usr/local/bin/greet` with a body that uses the build `ARG`;
- `COPY <<'RAW' <<-TWO /etc/conf/` with a quoted marker and a tab-indented `<<-` one.

In the built template `greet` runs and prints `hello world` (the unquoted body was expanded), its mode
is 755, `/etc/conf/RAW` holds `raw $WHO 'q'` unexpanded, `/etc/conf/TWO` has its tabs stripped, and
`ENV` after them still applies.

How it works: no upload. Each body becomes a `RUN` that makes the directory and writes the file from
base64 with `/bin/busybox base64 -d`, then `chmod`/`chown` if given. A destination ending in `/`
gets a file named by the marker.

## Not verified

- An image without `/bin/busybox` (the write fails the build).
- `COPY <<EOF` mixed with ordinary file sources (refused), `--chown` (unit-tested only), and `ADD <<EOF`
  (refused).
- Bodies with a trailing line lacking a newline, binary content, or `\`-escapes in an unquoted body
  (variable expansion follows this parser's `substitute`, not Docker's exactly).
- `SHELL` has no effect on these writes.
