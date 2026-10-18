# The tools image built by CI, booted on KVM (2026-10-18)

**What this checks:** that the image the `Tools image` workflow builds on a
GitHub runner (`debootstrap` and apt there, not on a developer machine) is
the same working template as the one built locally. The workflow itself only
looks inside the archive; this boots it.

## Method

Workflow run 39098263037 (commit `c00e8f34`) built `hm-tools.cpio.gz`, 187,467,551
bytes, SHA-256 `969c891e...c387a`. It was downloaded with `gh run download`,
its SHA-256 compared with the `.sha256` the workflow wrote (equal), and used as
`HV2_INITRD` for `tools/check-tools-image-kvm.py --network --memory-mb 2048`
on real KVM (WSL2, nested), the same check as
[the local build's](../../2026-10-16/tools-image-kvm/README.md).

## Result

All 5 cases passed, one run: Ubuntu 24.04 with git, Python, Node.js, npm, gcc,
make, curl and jq; offline work with them; from inside, with the network on,
`curl` over HTTPS, `git clone`, `npm install` and `apt install`; a fork that
carries the files and the tools; and the create-time and memory cost.
`report.json` has the details and the input hashes (the initrd's matches the
artifact's).

## Not verified

- One run, not three. Times in the report are the check's stopwatch on a busy
  host, not a benchmark.
- Whether the image is bit-for-bit reproducible between CI runs: apt installs
  whatever the archive holds that day, so two builds differ.
- Publishing: no release is tagged, so nothing is hosted for others to fetch.
