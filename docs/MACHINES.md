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
| `cpuCount`, `memoryMB` | the node's defaults | |
| `diskGiB` | 8 | 1–2048. The image is sparse, so it costs only what is written. |
| `autostart` | true | Start it again when the daemon starts, if it was running. |
| `restartPolicy` | `always` | `always` boots it again when its guest reboots or crashes, at most 5 times in 5 minutes; `never` leaves it stopped. |
| `start` | true | Start it once created. |
| `network` | none | Give it a NIC. See [Networking](#networking). Without it the machine has no network device at all. |

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

- **Firmware boot (UEFI) and stock cloud images or ISOs.** Machines boot the
  node's kernel today.
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
