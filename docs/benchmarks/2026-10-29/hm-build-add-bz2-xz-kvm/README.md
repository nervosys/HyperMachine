# `ADD` of a `.tar.bz2` and a `.tar.xz` (KVM)

`tools/check-hm-build-kvm.py` gained a twelfth case; an owned `hv2-sandboxd` with a guest kit, no
Docker. **3 of 3 runs passed all 12 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The context holds `b.tar.bz2` and `x.tar.xz` (made with Python's `tarfile`, one file `f.txt` each).
`ADD b.tar.bz2 /srv/b` and `ADD x.tar.xz /srv/x` build on `alpine:3.20`, and the template's
`/srv/b/f.txt` and `/srv/x/f.txt` hold each archive's own text. The guest's tar (the image's, or
`/bin/busybox tar`) recognised both compressions by itself.

## Not verified

- An image whose tar lacks bz2 or xz support and has no busybox (the unpack fails the build).
- An image with no `tar` and no busybox at all.
- Ownership, modes and symlinks inside an archive.
