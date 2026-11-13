# Template builds on a shell-free guest, on KVM

Follow-up to `../disk-mount-agent-kvm/README.md`. Template builds (`crates/hv2-sandboxd/src/builds.rs`)
were the last node-initiated guest steps that ran through `/bin/sh`. COPY, WORKDIR, USER and the base
environment now use the guest agent's operations (`read_file`, `write_file`, `stat`, `make_dir`, `remove`,
`set_attr`, `make_symlink`), and keep the shell script only on `hv2_agent::is_unsupported_op` (an agent that
predates them), on an archive or option the agent ops cannot express, or when the guest's own program is the
point (RUN).

| Step | Agent path | Shell, and when |
| --- | --- | --- |
| COPY | the repacked tar is parsed on the host (`env_vars::members_of`, shared with the repository unpack: `..` and a member written through an earlier symlink member are refused); directories `make_dir` + `set_attr` mode, files `write_file` + `set_attr` mode (an existing non-directory at the path is removed first, as `tar` replaces it), links `make_symlink`; then `set_attr` owner and/or octal `--chmod` on every listed path, non-recursive | old agent; hard link or special file in the archive; `--chmod` that is not octal (`u+x`); `--chown user:group`; an owner not in `/etc/passwd` (`owner_for_agent`: the agent resolves owners through passwd only, the shell's `chown` also takes a bare uid) |
| WORKDIR | `make_dir` (parents) and, for a non-root user, `set_attr` owner (user must be in `/etc/passwd`) | old agent; user not in passwd |
| USER | `/etc/passwd` read with `read_file`; a user present by name or uid needs nothing run | user absent: the useradd/adduser script |
| base environment (for `ENV $PATH`) | `busybox env` first; if that cannot start, `/proc/self/environ` and `/proc/self/stat` read with `read_file` | none |
| RUN | none (the step type carries only a shell-form command, no argv form) | always; a guest with no `/bin/sh` fails with a clear message |

Equivalence notes (also in the code): the agent sets the owner's primary group too (`chown user` kept the
group); a symlink's mode is not changed (`chmod` would change its target's); setuid/setgid bits of a file are
dropped; modification times are not kept. `/proc/self/environ` is the environment the agent was *started* with;
as PID 1 the agent then sets `PATH=/bin` and `HOME=/root` itself, which that file does not show, so the fallback
adds them when `/proc/self/stat` says the agent is PID 1, then the template's defaults over it, exactly the
layering `busybox env` shows (agentd: children inherit the agent's environment, plus `HOME`/`USER`/`LOGNAME` for
a non-root user, plus the template defaults, plus the request's variables; `busybox env` runs as root, so no
user variables).

`tools/check-build-noshell-kvm.py` (new) runs an owned daemon whose base template is the image under test and
drives the template-build API directly (`fromTemplate: base`; `--guest-kit` is an empty directory, nothing is
pulled), with `RUST_LOG=info,hv2_sandboxd::builds=debug`. Cases:

- a. COPY of a file with `--chmod 640`; COPY of a directory holding a 0750 subdirectory, a 0755 script, a file
  symlink and a directory symlink; ENV with `PATHX=/x:$PATH`; WORKDIR as root; USER `app` (found in the
  `/etc/passwd` the build copied in); COPY owned by `app`; WORKDIR as `app`. Read back through a checkpoint of a
  sandbox made from the template (files, modes, uid/gid, link targets from the archive route), no
  `.hv2-copy` file in `/tmp`, `GREETING` and `PATHX == "/x:" + PATH` in the sandbox's `/proc/self/environ`
  (exec by argv), and its working directory `/srv/work/sub`.
- b. a RUN step: shell image, the build succeeds and the file is there; shell-free, the build fails at step 2
  with a message naming `/bin/sh` and RUN.
- c. COPY owned by a user `/etc/passwd` does not hold: goes to the shell. Shell image: the shell's `chown` fails the build at step 1 (the command names `ghost`). Shell-free: fails saying the COPY needs the shell.
- d. COPY `--chmod u+x`: shell image, built, mode 744; shell-free, fails saying the COPY needs the shell.
- f. COPY owned by the bare uid `4242`, which passwd lacks: shell image, built through the shell path and the file is owned by uid 4242; shell-free, fails saying the COPY needs the shell.
- e. USER of a user the image lacks: shell-free, fails saying the image has no shell to create it; shell image,
  the script runs (this test busybox has no `adduser` applet, so it exits 127; that is the shell path's own
  behaviour, unchanged).

The driver then counts the daemon log's lines and requires exactly these numbers (6 builds per run): COPY
`using agent op` 5, COPY `falling back to shell` 3 (cases c, d, f), WORKDIR agent 2, shell 0, USER agent 1 (found in
passwd), USER shell 1 (case e), base environment from `busybox env` 6 on the busybox image and 0 on the
shell-free one, from `/proc/self/environ` 0 and 6, and 0 lines mentioning `Tcg`.

Three consecutive runs (`run1` to `run3`) on each of two images, all pass 6 of 6 with the counts above. Each
`runN/<image>/` holds `report.json` (cases, the build logs, counts, input hashes) and `daemon-log.txt`.

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V).
- Daemon: Linux release `hv2-sandboxd` built from the working tree on repo commit `831e7c72` plus the
  uncommitted change under test, sha256 `0cf628784805b80eb385c1e693b4ec5488b0a1b994f8d3ccce9748326a2e4fb3`,
  including `owner_for_agent` (a passwd check before the agent is asked to own anything).
- `hm` from the same build, sha256 `f98c086c9c58ed9026eaade027ff7e3acd6ff9c9317d106ef96fbfe5e33ddd4e`.
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Images:
  - `shell-free`: `noshell-tools.cpio.gz`, sha256 `acb882968a8659ddcb332d28dd7004d21e65078e42dd6ae6fb8c219c19de36ec`
    (the new agent as PID 1, no `/bin/sh`, no busybox, five copies of busybox as `/bin/cat`, `dd`, `sha256sum`,
    `grep`, `ls`; built as in `../disk-mount-agent-kvm/README.md`).
  - `busybox`: busybox plus the new agent, sha256
    `12577fb386d9649fbea4961478c4279812ab594e27fbad028e37c5342b837bd9`.
  - `old-image` (regression only): busybox plus the agent from before the file operations, sha256
    `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`.
- Driver `tools/check-build-noshell-kvm.py` sha256
  `e3a23c3cf559258a7b477b9c24496aea78fca1688cca1760278fd4a0900e81b0` (also each `report.json`).
- Daemon flags: `--guest-kit <empty dir> --memory-mb 512 --cpu-cores 1 --capacity 6`.

Hypervisor lines in each daemon log: `Detected hypervisor platform: Kvm` 18 (shell-free) and 23 (busybox) per
run, `Tcg` 0, in all six runs (recorded in `report.json` as `log_counts`).

## Results (identical in all three runs)

| Case | shell-free | busybox (new agent) |
| --- | --- | --- |
| a. COPY, ENV, WORKDIR, USER, COPY owned | ok | ok |
| b. RUN | fails at step 2: `a RUN step runs through /bin/sh: this guest's image has no /bin/sh, and this route runs its command through one (...)` | built, `/ran.txt` present |
| c. COPY owned by a missing user | fails at step 1: `this COPY needs the shell (owner ghost is not in /etc/passwd): this guest's image has no /bin/sh, ...` | the shell's `chown -h 'ghost'` fails the build (the failing script is the message) |
| d. COPY `--chmod u+x` | fails: `this COPY needs the shell (--chmod "u+x" is not an octal number): this guest's image has no /bin/sh, ...` | built, mode 744 |
| f. COPY owned by bare uid 4242 (not in passwd) | fails: `this COPY needs the shell (owner 4242 is not in /etc/passwd): ...` | built via the shell path, file owned by uid 4242 |
| e. USER of a missing user | fails: `user builder is not in the image's /etc/passwd and the image has no shell to create it: ...` | script ran, `exited 127` (no `adduser` applet in this busybox) |
| log counts: COPY agent / shell | 5 / 3 | 5 / 3 |
| WORKDIR agent / shell | 2 / 0 | 2 / 0 |
| USER agent / shell | 1 / 1 | 1 / 1 |
| base env: `busybox env` / `/proc/self/environ` | 0 / 6 | 6 / 0 |

In case a, `PATH` read in the sandbox was `/bin` on the shell-free image (so `PATHX` was `/x:/bin`, which
came from the `/proc/self/environ` fallback plus the PID 1 defaults).

## Regression: `tools/check-hm-build-kvm.py` (`regression/`)

The existing build check (real `hm`, `alpine:3.20` pulled by the node, a guest kit, the node's network for
the pull), one run per image, same daemon, `hm` and kernel; the kit's agent matches the image's:

| Image | Kit | Result |
| --- | --- | --- |
| busybox, new agent | busybox, new agent, `init` from that image | `success: true`, 12 of 12 cases ok; 26 `Kvm` lines, 0 `Tcg` |
| old-image (old agent) | busybox, old agent, same `init` | `success: true`, 12 of 12 cases ok; 26 `Kvm` lines, 0 `Tcg` |

The check hard-codes `RUST_LOG=info`, so its logs do not show which path each step took; only the pass is
claimed. The old-agent run exercises the shell fallback only in so far as the unchanged logic reaches it: the
agent ops' `is_unsupported_op` mapping on an old agent was not confirmed from a log line here.

## Rust sweep

- Windows (rustc 1.98.1): `cargo fmt --check` clean; `cargo clippy --all-targets -p hv2-sandboxd -- -D warnings`
  clean; `cargo test -p hv2-sandboxd`: `test result: ok. 151 passed; 0 failed; 2 ignored` (run before `owner_for_agent` was added; the coordinator reports the same 151 passed after it, not rerun here).
- WSL (rustc 1.100.0, `RUSTFLAGS="-D warnings"`, current tree including `owner_for_agent`): clippy `--all-targets` clean; `cargo test -p hv2-sandboxd`: `test result: ok. 216 passed; 0 failed; 2 ignored`.

New unit tests (`builds::tests`): octal mode parsing, passwd lookup by name and uid (incl. malformed lines),
`/proc/self/environ` parsing and the PID 1 defaults, the no-shell start message, a COPY archive read as members
(directory, file, symlink), refusal of a member written through an earlier symlink, and a hard link going to
the shell path.

## NOT VERIFIED

- Hard links and special files in a COPY archive going to the shell path is unit-tested only, not run on a
  guest; likewise `--chown user:group`.
- The symlink-escape refusal is unit-tested (`members_of`), not provoked through a real build upload.
- An old agent's `is_unsupported_op` fallback in builds (COPY/WORKDIR) was not observed in a log: the
  old-image `check-hm-build-kvm.py` run only shows the builds pass; the new driver was not run on the old
  image.
- Setuid/setgid bits, modification times and group ownership differences listed above were not compared
  against the shell script on a guest.
- A COPY of many files (the cost of one `stat` plus `write_file` plus `set_attr` per file, plus one passwd read) was not timed; no
  timing claim.
- `RUN` has no argv form in the step schema, so none was added; RUN on a shell-free guest only fails clearly.
- `COPY` with a `uid:gid` owner, and USER given as a numeric uid with no passwd file, were not tried (a bare
  numeric COPY owner not in passwd is case f).
- The Windows/macOS builds of `hv2-sandboxd` compile this code (clippy on Windows), but only Linux ran it.
