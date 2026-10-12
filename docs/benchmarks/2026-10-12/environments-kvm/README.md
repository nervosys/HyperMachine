# Environments: variables and files a sandbox starts with

**What this checks:** [environments](../../../ENVIRONMENTS.md), a named set
of variables and files applied to new sandboxes. boat has environments with
variables, files and repositories; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-environments-kvm.py` runs an owned `hv2-sandboxd` with a
template. The environment `web` has three variables, one of them a token,
a text file and a binary file of 1,029 bytes holding every byte value.

1. **Made.** It must answer with the variables' names and the files' paths
   and sizes, and neither it nor the listing may hold the token or the text
   file's contents.
2. **Started with it.** A sandbox created with `web` and its own `MODE`: its
   commands must see the environment's `API_URL` and its own `MODE`, read
   the text file, and read the binary file back byte for byte. The files
   must be owned by the user commands run as.
3. **Started without it.** A sandbox created without it has none of it.
4. **Reboot and fork.** The first sandbox is rebooted, which starts its
   guest again from the template: the variables and files must be back. A
   fork of it must have them too.
5. **A change.** `web` is replaced with another `API_URL` and another text
   file and no binary one. A sandbox created after has the new; the one
   already running still has the old.
6. **Refusals.** An environment that is not there; a relative path, a path
   with `..`, a variable name that is not one, a name that is not one, a
   field that is not one, none of them kept; and after `web` is deleted, a
   create naming it.

The node's log must not hold the token.

## Result

All six passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-12. [`report.json`](report.json) has what each case saw.

The first run of the check stopped in case 4, in the check: it read the
fork's answer in the wrong shape. The reboot had passed.

## What this does not show

- **A template user that is not root.** The base template's commands run as
  root, so the files were root's and owning them as the template's user was
  not told apart from owning them as root.
- **Teams.** The node ran with no keys. Each team keeping its own is
  unit-tested, two teams with one name included.
- **A cluster store**, and the control plane serving them.
- **An environment's limits on KVM.** They are unit-tested.
