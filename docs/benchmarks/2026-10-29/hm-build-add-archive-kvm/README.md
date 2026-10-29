# `ADD` of a local archive in `hm sandbox vm build` (KVM)

`tools/check-hm-build-kvm.py` gained a ninth case; an owned `hv2-sandboxd` with a guest kit, no
Docker. **3 of 3 runs passed all 9 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The context holds `data.tar.gz`, `plain.tar` (each with `inner/a.txt` and `b.txt`) and a file named
`site.zip`. The Dockerfile has `ADD data.tar.gz /srv/data`, `ADD plain.tar /srv/plain/` and
`ADD site.zip /srv/site.zip`. In the built template the archives' files are unpacked under
`/srv/data` and `/srv/plain`, the zip is there whole, and no `/tmp/.hm-add-*` file is left.

How it works: the archive is copied to `/tmp/.hm-add-N.tar`, a `RUN` unpacks it with the image's
`tar` (or `/bin/busybox tar` if the image has none) into the destination directory, and removes it.
Docker unpacks tar, tar.gz, tgz, tar.bz2 and tar.xz and copies anything else, a zip included.

## Not verified

- bz2 and xz archives (only plain and gzip ran; they depend on the guest's tar).
- An image with no `tar` and no busybox, where the unpack fails the build.
- Ownership, modes and symlinks inside an archive; `--chown` and `--chmod` on an `ADD` archive
  (the flags apply to the copied archive, not the unpacked files).
- `ADD` of a URL or a git source is still refused.
