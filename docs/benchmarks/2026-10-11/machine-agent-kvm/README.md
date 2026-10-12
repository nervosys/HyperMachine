# exec in an Ubuntu machine: the guest agent, installed by cloud-init

**What this checks:** that a machine from a distribution's image can run
this project's guest agent, so that `POST /machines/{name}/exec` works in it
as it does in a machine from a template. Until now the only way into such a
machine was its serial console. boat's sandbox takes `boat exec`; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings worth reading and compares
with nothing.

## The machine

`ubuntu-24.04-minimal-cloudimg-amd64.img` as published (sha256
`2f119f50…a777df`), converted to raw and not otherwise changed. Created with
four processors, 2 GiB, user data setting a password for `ubuntu`, and
`"guestAgent": true`. No network. Firmware: Rust Hypervisor Firmware 0.5.0.

The node was started with `--guest-agent` naming `hv2-guest-agentd` built in
release for `x86_64-unknown-linux-gnu`: 663,800 bytes, dynamically linked,
which Ubuntu's own C library runs. Its hash is in
[`report.json`](report.json).

## How the agent gets there

Nothing in the image is changed and nothing is typed. The node puts two more
files on the machine's [cloud-init seed](../../../FIRMWARE_BOOT.md#a-user-at-creation-the-cloud-init-seed):
the agent's binary, and `vendor-data`, a shell script that copies the binary
to `/usr/local/sbin`, writes a systemd unit for it, and enables and starts
it. cloud-init runs that script once for the instance. The agent listens on
`AF_VSOCK`; the node gives the machine a vsock device on the PCI bus, which
Ubuntu's kernel has a driver for, and waits for the agent to answer.

## Method

`tools/check-machine-distro-kvm.py --user ubuntu --sudo --user-data FILE
--guest-agent FILE`. Its cases are those of the cloud-init check, with one
added after cloud-init is done and one assertion added to the last:

- **exec answers.** `POST /machines/{name}/exec` must run a command, having
  answered 409 with a reason until then. Through it: the user is `root`, the
  first process is `systemd`, the processors are the four it was given, the
  agent's unit is `active` and `enabled`. A file written through `exec` is
  read at the console and one written at the console is read through `exec`,
  so both are the same guest. `echo oops >&2; exit 7` comes back as exit code
  7 with `oops` on standard error.
- **After power off and start**, `exec` must answer again, with the agent's
  unit active and cloud-init having run no second time.

## Result

All seven cases passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-11.

| | |
|---|---|
| Through `exec` | `root`; kernel 6.8.0-146-generic; first process `systemd`; 4 processors |
| The agent's unit | `active`, `enabled` |
| cloud-init's vendor scripts for the instance | 1, run once |
| A file across `exec` and the console | read each way |
| `echo oops >&2; exit 7` | exit code 7, `oops` on standard error |
| After power off and start | `exec` answers; same instance ID, one instance recorded |
| The node's log | `its guest agent answered`, once for each boot |

`exec` had not been refused by the time the check first tried it, on either
boot: the check logs in and waits for cloud-init first, and the agent was up
before that was done. So this run does not show the 409 that comes before
the agent answers, only that the wait for it ended.

## What it took

The second run. In the first, the guest did everything right: cloud-init ran
the script, the unit was active, the agent logged that it was listening, the
vsock driver was loaded. The node never heard from it. A machine from an
image was built without the capability that lets the node talk to a guest
agent at all, which a machine from a template is built with, so every ping
was refused on the node's own side and the wait went on asking. The
capability is now given when the machine asks for an agent, and a refusal of
that kind ends the wait with an error in the log.

## What this does not show

- **Files through the API.** A machine has `exec` and a console; it has no
  file routes of its own, with or without an agent.
- **An image without cloud-init or without systemd.** The install depends on
  both. Such an image would boot and never answer.
- **A statically linked agent.** The binary here needs the guest's C
  library, which Ubuntu has. A guest with another libc needs the musl build.
- **A machine with both a network and an agent.** Each was run; not both in
  one machine. They share no device or interrupt line.
- **What the agent can be asked to do beyond `exec`**, or how the node's
  stop behaves with an agent in such a guest: a stop now asks it to `sync`
  first, and that was not observed.
