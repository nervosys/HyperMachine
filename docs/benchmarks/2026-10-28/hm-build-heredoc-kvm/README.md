# `RUN` here-documents in `hm sandbox vm build` (KVM)

`tools/check-hm-build-kvm.py` gained an eighth case; an owned `hv2-sandboxd` with a guest kit, no
Docker. **3 of 3 runs passed all 8 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The Dockerfile has `ARG WHO=world`, `RUN cat <<EOF > /note.txt` (body `hello $WHO` and an
indented line), `RUN <<EOT` (a `set -e` script writing `/script.txt`) and an `ENV` after them.
The template's files hold exactly `hello world`, `  indented`, `from-script`, and the `ENV`
applies, so the parser keeps the body, the terminator ends the instruction, and the next line is
read normally.

How it works: the body and terminator stay in the `RUN` command and the guest's `/bin/sh` reads
the here-document; a bare `RUN <<EOT` becomes `/bin/sh <<EOT ...`. `<<-` strips leading tabs from
the terminator. Unit tests cover `<<-`, an unclosed marker and `COPY <<` (refused).

## Not verified

- `SHELL` together with `RUN <<EOF` (the bare form always uses `/bin/sh`).
- A body line that starts with `FROM` in a multi-stage file (stages are split before lines are
  joined, so this may mis-split).
- Quoted markers (`<<'EOF'`) reach the shell as written, but no KVM case runs one.
- `COPY <<EOF` and `ADD` of a URL or archive are still refused; so are `RUN --mount` and `ONBUILD`.
