# Machines

A machine is a long-lived VM, as a vSphere host runs them. It is the other
kind from a sandbox:

| | Sandbox | Machine |
|---|---|---|
| Root filesystem | RAM (an initramfs), gone when it stops | its own ext4 disk, kept |
| Lifetime | capped (24 h) | none |
| A guest reboot or crash | rebooted fresh from its template | booted again from its disk |
| The daemon restarting | the sandbox is lost | the machine starts again by itself (`autostart`) |
| Starts in | milliseconds (snapshot restore) | seconds (a cold boot from disk) |

Machines are served by Linux nodes (`hv2-sandboxd`) with KVM. Each machine lives
in a directory under `--machine-dir`, which must be durable storage on that
node. The default is `<temp>/hv2-sandboxd-machines`. The directory holds
`machine.json` (what the machine is and whether it should be running) and
`root.img`.

## Using one

```sh
# A machine from the base template: 2 vCPUs, 2 GiB, a 20 GiB root disk, running.
curl -s -X POST localhost:3980/machines \
  -d '{"name":"web-01","templateID":"base","cpuCount":2,"memoryMB":2048,"diskGiB":20}'

curl -s -X POST localhost:3980/machines/web-01/exec -d '{"cmd":"df -h /"}'
curl -s -X POST localhost:3980/machines/web-01/stop
curl -s -X POST localhost:3980/machines/web-01/start
curl -s localhost:3980/machines/web-01/console      # the end of its serial console
curl -s localhost:3980/machines/web-01/network/decisions   # what its gateway allowed and refused
curl -s -X DELETE localhost:3980/machines/web-01     # stopped machines only
```

The same with `hm`, against a node or the control plane (`--endpoint`, or
`HV2_SANDBOX_URL`; `HV2_API_KEY` or a `login` session):

```sh
hm sandbox vm machine create web-01 --cpus 2 --memory-mb 2048 --disk-gib 20     --allow-out api.example.com --deny-out 10.9.0.0/16
hm sandbox vm machine list
hm sandbox vm machine exec web-01 -- df -h /     # exits with the command's code
hm sandbox vm machine stop web-01                # also: start, restart, inspect
hm sandbox vm machine console web-01
hm sandbox vm machine decisions web-01
hm sandbox vm machine delete web-01
```

`--network`, `--allow-out`, `--deny-out` or `--no-internet` each give the machine
a NIC; with none of them it has no network device. `--node` picks the node when
talking to a control plane.

| Field on create | Default | |
|---|---|---|
| `name` | required | Letters, digits, `-` and `_`, up to 63 characters. |
| `templateID` | `base` | Any template this node has, including one built from an OCI image. Its file tree becomes the root disk. |
| `image` | none | In place of `templateID`: a raw disk image in the node's `--image-dir`, booted by firmware. See [From a disk image](#from-a-disk-image). |
| `userData` | none | With `image` only: cloud-init user data for the guest, a `#cloud-config` document or a script, up to about 1 MiB. See [firmware boot](FIRMWARE_BOOT.md#a-user-at-creation-the-cloud-init-seed). Refused for a machine from a template. Not returned afterwards; the machine's record carries `cloudInit: true`. |
| `guestAgent` | false | With `image` only: run HyperMachine's guest agent in the guest, so `exec` works. Refused on a node without `--guest-agent`, and for a machine from a template, which always runs it. |
| `cpuCount`, `memoryMB` | the node's defaults | `cpuCount` is 1 to 32, and refused otherwise. From an image it is 1 unless given |
| `diskGiB` | 8 | 1–2048. The image is sparse, so it costs only what is written. |
| `autostart` | true | Start it again when the daemon starts, if it was running. |
| `restartPolicy` | `always` | `always` boots it again when its guest reboots or crashes, at most 5 times in 5 minutes; `never` leaves it stopped. |
| `start` | true | Start it once created. |
| `network` | none | Give it a NIC. See [Networking](#networking). Without it the machine has no network device at all. |

## From a disk image

A machine can be made from a raw disk image in place of a template. It then
boots by [firmware](FIRMWARE_BOOT.md): the firmware finds the disk, and the
image's own bootloader and kernel start, unchanged. This is how a stock cloud
image runs.

```sh
# On the node: where the firmware and the images are.
hv2-sandboxd --machine-dir /var/lib/hm/machines \
    --firmware /usr/share/hm/hypervisor-fw --image-dir /var/lib/hm/images

curl -s -X POST localhost:3980/machines -d '{"name":"cloud-01","image":"cirros.raw"}'
hm sandbox vm machine create cloud-01 --image cirros.raw       # the same

hm sandbox vm machine console cloud-01 --tail 2000    # what the guest printed last
hm sandbox vm machine type cloud-01 cirros            # type at its serial console
```

- **`image`** is a file name in the node's `--image-dir`. The file is copied to
  the machine's `root.img`, so the machine owns its disk and the image is not
  changed. `diskGiB` may be larger than the image; the file is then grown, and
  an image that resizes its filesystem at first boot uses the space. It may not
  be smaller.
- **A guest agent, when asked for.** A stock image does not run HyperMachine's
  agent. Create the machine with `guestAgent: true`
  (`hm sandbox vm machine create --guest-agent`) on a node started with
  `--guest-agent FILE`, and the node installs it through the image's
  cloud-init and gives the machine a channel to it; `exec` then works. The
  machine counts as running once its vCPU is, which is before its agent
  answers, and `exec` answers 409 until it does. The image must run
  cloud-init and systemd, and the binary must run on its C library. See
  [firmware boot](FIRMWARE_BOOT.md#a-guest-agent-in-the-image).
- **The serial console is always there:** `GET /machines/{name}/console?tail=N`
  returns its last N bytes (up to 1 MiB), and `POST /machines/{name}/console`
  types the request body at it. Without an agent it is the only way in.
- **Stopping** such a machine from the API stops the VM without asking the
  guest to shut down, as pulling the plug would. With an agent the node asks
  it to `sync` first; without one, shut the guest down or `sync` from its
  console.
- **A guest that turns itself off stays off.** `poweroff` in the guest stops
  the machine, and it is left stopped whatever its `restartPolicy`: that
  policy restarts a guest that stopped by itself, a crash or a `reboot`, and
  a guest that turned itself off asked for the opposite. Start it again
  through the API.
- **A network** works as it does for any machine: give `network` on create, and
  the machine gets one NIC behind the node's egress gateway, with the same
  `allowOut` and `denyOut` rules. The NIC is on the PCI bus, where a stock
  operating system looks, and the guest configures itself by DHCP: the gateway
  answers with the guest's address, the default route and the resolver. Nothing
  else is set up in the guest, since there is no agent to do it; the egress CA
  for HTTPS interception is not installed.
- **A user, a key, a hostname:** give `userData`, and the image's cloud-init
  applies it at first boot with the machine's name as the hostname.
- **Limits today:** raw images only, and the firmware is not shipped with
  HyperMachine. See [firmware boot](FIRMWARE_BOOT.md).

## Through the control plane

The control plane serves the same `/machines` routes, so a cluster's machines
are managed in one place:

```sh
curl -s -X POST https://control.example/machines -H "X-API-Key: $KEY" \
  -d '{"name":"web-01","diskGiB":20}'
curl -s https://control.example/machines -H "X-API-Key: $KEY"
```

- **Placement.** A new machine goes to the live node with the most free
  capacity that offers its template, or to the node `nodeID` names. The reply's
  `x-hv2-node` header and each listed machine's `nodeID` say where it is. A
  machine stays on the node that holds its disk.
- **Finding it.** No store records where a machine is: the control plane asks
  the nodes on each request. So a control plane restart loses nothing. While a
  node is not answering, a machine that is not found answers 503 rather than
  404, and creating one is refused, since its name may be taken on that node.
- **Access.** A key needs the `machines` scope (or `admin`). An `inventory` key
  and an `observer` role do not reach machines.
- **Teams.** A machine belongs to the [team](TEAMS.md) of the key that created
  it. Its ID is derived from the team and the name, so two teams can each have
  a machine called `web-01`. A team reaches its own machines by name; another
  team's answer 404, by name and by ID. An administrator lists every machine
  and reaches a team's by its ID.

## Networking

A machine created with a `network` object has one NIC, behind the same egress
gateway a sandbox's NIC is behind. The gateway is a userspace router in the node
daemon that decides every connection the guest makes. There is no bridge or TAP
device on the host, and nothing on the network can connect in to the machine.

```sh
curl -s -X POST localhost:3980/machines -d '{"name":"web-01",
  "network":{"allowOut":["api.example.com","10.20.0.0/16"],"denyOut":["10.20.9.0/24"]}}'
```

| Field of `network` | |
|---|---|
| `allowInternetAccess` | `false` refuses everything `allowOut` does not name. |
| `allowOut` | Host names, addresses and CIDRs it may reach. |
| `denyOut` | Addresses and CIDRs it may not. |

The rules mean what they mean for a sandbox:

- An empty `network` (`{}`) takes the node's default, which is deny unless the
  operator set `--egress-default allow`.
- Private, link-local and cloud metadata addresses are refused whatever the
  rules say, unless the operator granted the range with `--tenant-reserved-cidr`.
- The network is stored with the machine and decided again at every boot.
  `GET /machines/{name}/network/decisions` shows what the gateway allowed and
  refused since the last boot.

The guest's address, route and resolver come from the kernel command line, so
the guest needs no DHCP client. On a node run with `--network`, the egress CA is
added to the guest's trust bundle at each boot, once: a boot that finds it there
leaves the bundle alone.

## Runs

`exec` waits for a command and answers with what it printed. A run is for a
command that takes longer than anyone wants to wait: it is started and left,
and the node keeps what it prints.

```sh
# Start one. The answer comes at once, with its ID.
curl -sS -X POST $NODE/machines/web-01/runs -H 'content-type: application/json' \
  -d '{"cmd":"make -C /srv/app test","cwd":"/srv/app","env":{"CI":"1"}}'
# → {"runID":1,"state":"running","pid":412,...}

# Read it. `next` is where to read from the next time.
curl -sS "$NODE/machines/web-01/runs/1?stdout=0&stderr=0"
# → {"state":"running","stdout":{"offset":0,"text":"...","next":2048,"dropped":0},"stderr":{...}}

curl -sS -X POST $NODE/machines/web-01/runs/1/stdin  -d '{"data":"yes\n"}'
curl -sS -X POST $NODE/machines/web-01/runs/1/signal -d '{"signal":"INT"}'
curl -sS $NODE/machines/web-01/runs          # every run kept, oldest first
```

| | |
|---|---|
| `cmd` | A shell command line, run by the guest's `/bin/sh -c`. |
| `cwd`, `user` | Where it starts, and as whom; the agent's own directory and root unless given. |
| `env` | Set for the command. Not kept and not returned: it is where a key goes. |
| `stdin` | Written to its standard input, which is then closed. Leave it out to keep it open for `.../stdin`. |

- **A run ends** as `exited`, with an exit code or the signal that ended it,
  or as `lost` with the reason when the node can no longer reach it. A
  machine that stops takes its programs with it, and their runs are `lost`.
- **Output is kept on the node, in memory**: the newest 4 MiB of each stream,
  with `dropped` counting what went before. It outlives the run and the
  machine being stopped, and is gone when the node restarts or the machine is
  deleted. 256 runs are kept for a machine; the oldest finished one makes
  room, and with 256 still running a new one is refused.
- **It needs the guest agent**, as `exec` does: a machine from a template
  always has it, and one from an image has it when created with
  `guestAgent`.
- **It has pipes, not a terminal.**

[Checked on real KVM](benchmarks/2026-10-11/machine-runs-kvm/README.md).

## How it boots

At creation, the template's initramfs is unpacked into a fresh sparse ext4 image
with `mkfs.ext4 -d`. Device nodes are skipped, since the guest's init mounts
devtmpfs. The guest boots the node's kernel with no initramfs and
`root=/dev/vda rw rootfstype=ext4 init=/init`, from that image attached as a
virtio-blk disk. Every write goes to the image before the guest sees it complete,
and a guest flush is an `fsync`.

Stopping a machine syncs its filesystems through the guest agent, then stops the
VM. There is no ACPI power button yet, so a guest's own `poweroff` halts the guest
without the VM exiting. Stop a machine through the API.

## Not yet

- **ISO installers.** They need a firmware that has not been run here yet.
- **A machine from a template that turns itself off.** Its kernel is this
  project's, with no power registers named in its tables, so `poweroff`
  there still halts the guest without the VM exiting.
- **Networking beyond egress.** A machine cannot be reached from the network:
  there is no inbound port forwarding, no bridged or VLAN networking, and no
  second NIC. Its network cannot be changed after creation, and it does not join
  private sandbox networks.
- **Formats.** QCOW2, and several disks per machine.
- **Control plane.** It routes `/machines`, but there is no event or webhook
  when a machine changes state, and each request asks every node where the
  machine is, which will not suit a large fleet.
- **Migration.** Moving a machine between nodes, live migration, and HA restart
  on another host.

These are tracked in the [VMware replacement plan](VMWARE_REPLACEMENT.md).

Evidence:
- [real KVM: boot from disk, stop/start, guest reboot, daemon kill, delete](benchmarks/2026-10-07/machines-kvm/README.md)
- [real KVM: a machine's NIC, allowed and refused egress, the network after a restart and a reboot](benchmarks/2026-10-08/machine-network-kvm/README.md)
- [real KVM: machines through the control plane, with two teams](benchmarks/2026-10-08/machines-cluster-kvm/README.md)
- [real KVM: a networked image machine, configured by DHCP, reaching what it may](benchmarks/2026-10-09/machine-image-network-kvm/README.md)
- [real KVM: a machine booted by firmware from a stock cloud image, driven over its console](benchmarks/2026-10-08/machine-firmware-kvm/README.md)
