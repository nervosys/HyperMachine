# Ubuntu as a machine with a network: apt, then Docker running containers

**What this checks:** that a machine made from Ubuntu's cloud image is a
place to install and run things: it configures its own network, installs a
package from the distribution's archive, and runs containers. boat's sandbox
is an Ubuntu 24.04 VM in which one can "run docker, systemd, databases, cron
inside"; see the [comparison](../../../BOAT_PARITY.md).

This is a functional check, run by hand, and it reaches the Internet. The
times it records are of one guest on a busy development host under nested
virtualization. They are not claims, and it compares with nothing.

## The machine

`ubuntu-24.04-minimal-cloudimg-amd64.img` as published (sha256
`2f119f50…a777df`), converted to raw and not otherwise changed. Created
through `POST /machines` with four processors, 2 GiB, a 10 GiB disk,
[cloud-init user data](../../2026-10-10/machine-cloud-init-kvm/README.md)
that sets a password for `ubuntu`, and

```json
"network": {"allowInternetAccess": true}
```

which gives it one NIC behind the node's egress gateway with the Internet
allowed. Reserved addresses stay closed, as for any guest. Firmware: Rust
Hypervisor Firmware 0.5.0.

## Method

`tools/check-machine-distro-kvm.py --user ubuntu --sudo --user-data FILE
--network --disk-gib 10`, on an owned `hv2-sandboxd`. Its cases are the six
of the cloud-init check, with three between login and power-off:

- **A network.** The guest must have an address and a default route, and
  resolve `archive.ubuntu.com`.
- **apt.** `apt-get update`, then `apt-get install -y docker.io`, must both
  exit zero, and `systemctl is-active docker` must say `active`.
- **Containers.** `docker run --rm hello-world` must print its greeting, and
  `docker run --rm alpine:3` must run a shell that reports itself as process
  1 in the container.

## Result

All nine cases passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-11. [`report.json`](report.json) has each answer.

| | |
|---|---|
| Address, route | `10.0.2.15/24` by DHCP, default via `10.0.2.2` |
| Root filesystem | 8.7G, grown by cloud-init into the 10 GiB disk |
| `systemctl is-system-running` | `running`, no job queued, no unit failed |
| `apt-get update` and `install docker.io` | both exit 0, in 45 s together; `docker.io 29.1.3-0ubuntu3~24.04.2` |
| Docker service | `active`; server 29.1.3, `overlayfs`, cgroup v2 |
| `hello-world` | "Hello from Docker" |
| `alpine:3` | a shell, process 1 in its container |
| Pulling and running both images | 20 s |
| From create to the login prompt | 36.0 s |
| virtio devices on the I/O APIC | 3: the disk, the seed, the NIC |
| Power off, start again | left off; then the same instance, the file kept |

The NIC also settled the two things the check without one recorded as not
what one would want: systemd ends `running` and not `degraded`, and the
login prompt comes in 36 seconds and not 145.

## What it took to get here

This run is the third. The first two stopped at `apt-get update`:

```
E: Release file for http://archive.ubuntu.com/ubuntu/dists/noble-updates/InRelease
is not valid yet (invalid for another 74d 4h 15min 51s).
```

The guest's clock was wrong. The emulated RTC had its data-mode bit
inverted, so Linux decoded binary registers as BCD and set its clock to
2020, and systemd then moved it up to its own build date in July. A guest
running this project's agent never showed that, because the agent sets the
clock. It is fixed in the same change as this record, and the guest in this
run logged `rtc_cmos: setting system clock to 2026-10-11T16:56:48 UTC`.

## What this does not show

- **A policy narrower than the whole Internet.** The machine was allowed
  all of it. Which names an `apt` and a `docker pull` need, and whether a
  list of only those is enough, was not tried.
- **HTTPS interception.** No egress secret or header rule was configured, so
  the gateway passed TLS through and the guest needed no extra certificate.
- **A guest agent.** Everything was typed at the serial console. `exec`
  still answers 409 for a machine from an image.
- **Docker under load**, a build, a volume, a published port, or a
  container reached from outside the machine. There is no way into a
  machine from the network yet.
- **Speed.** One guest, nested virtualization, a host doing other work.
- **Another distribution or firmware.**
