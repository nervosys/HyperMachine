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
| `agents` | none | With `guestAgent` only: coding agents to install at first boot, for [prompts](#prompts). `claude` (Claude Code) and `codex` (Codex CLI). Each is installed once, from npm, after the guest agent. `hm`: `--agent claude --agent codex`. |
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

`hm sandbox vm machine run web-01 --cwd /srv/app --pass-env CI -- make test`
starts one, and `... machine runs web-01 [ID]` lists them or shows one.

## Published ports

A machine's guest listens on a port; publishing it gives that port one of
the node's, and a connection to the node's port is carried to the guest's.

```sh
curl -sS -X POST $NODE/machines/web-01/ports -H 'content-type: application/json' \
  -d '{"guestPort":8080}'
# → {"hostPort":20000,"guestPort":8080,"address":"127.0.0.1","listening":true}

curl -sS http://127.0.0.1:20000/              # the guest's port 8080
curl -sS $NODE/machines/web-01/ports
curl -sS -X DELETE $NODE/machines/web-01/ports/20000

hm sandbox vm machine publish web-01 8080 [--host-port 20080]
hm sandbox vm machine ports web-01
hm sandbox vm machine unpublish web-01 20000
```

- **The operator decides which ports and which address.** A node gives out
  ports from `--machine-ports FIRST-LAST` (20000-29999 unless said) and
  listens for them on `--machine-port-address` (its loopback unless said). A
  caller picks a port in that range with `hostPort`, or lets the node pick.
  So publishing a port does not by itself put a guest on the Internet:
  that takes an operator starting the node with an address that is.
- **It goes in through the guest agent**, to the port on the guest's
  loopback. The machine needs no NIC for it, and one with a NIC keeps its
  egress rules as they are. It needs the agent: a machine from an image made
  without `guestAgent` is refused.
- **The node listens for as long as the port is published.** While the
  machine is stopped, or nothing in the guest listens on the port, a
  connection is accepted and closed. When the machine starts again the port
  works as it did.
- **Published ports are kept with the machine**: there after the node
  restarts, gone when the machine is deleted. At most 16 for a machine.
- **It is TCP, and the bytes are the guest's.** The node adds no TLS, no
  HTTP and no login on the published port itself; whoever can reach the
  node's port reaches the guest's.
- **By name, through the node's proxy**, with `"proxy": true` (`hm ...
  publish web-01 8080 --proxy`): the port is also reached at
  `{guestPort}-{machineID}.{domain}` through the node's sandbox proxy, as a
  sandbox's ports are, over TLS when the node has `--tls-cert`; and by E2B's
  `e2b-sandbox-id` and `e2b-sandbox-port` headers. The answer gives the
  name's first label as `proxyLabel`. Off unless asked, since the proxy is
  commonly on a public address. A node's own proxy adds no login.
- **With a login, through the control plane.** The control plane's proxy
  reaches a machine's proxied port too, at the same name, and puts its
  [operator-provisioned login](PRIVATE_GUEST_URLS.md) in front of it: grant
  the machine's ID (`machineID`) in the web access file as a sandbox's would
  be. [Checked on KVM](benchmarks/2026-10-12/machine-port-login-kvm/README.md).
- **Through the control plane** the routes are forwarded, and the port is
  on the machine's node, at that node's address.

[Checked on real KVM](benchmarks/2026-10-11/machine-ports-kvm/README.md).

## Snapshots

A snapshot is a stopped machine's disk, copied and kept beside it: files,
not memory. It is restored over the disk, or downloaded as a raw image.

```sh
curl -sS -X POST $NODE/machines/web-01/stop
curl -sS -X POST $NODE/machines/web-01/snapshots -d '{"name":"before upgrade"}'
# → {"snapshotID":"snap-61d5adc592c0469","name":"before upgrade","diskGiB":20,"bytesOnNode":5226496,...}
curl -sS $NODE/machines/web-01/snapshots
curl -sS -X POST $NODE/machines/web-01/snapshots/snap-61d5adc592c0469/restore -d '{}'
curl -sS $NODE/machines/web-01/snapshots/snap-61d5adc592c0469/disk -o web-01.img
curl -sS -X DELETE $NODE/machines/web-01/snapshots/snap-61d5adc592c0469

hm sandbox vm machine snapshot take web-01 --name "before upgrade"
hm sandbox vm machine snapshot list web-01
hm sandbox vm machine snapshot restore web-01 snap-61d5adc592c0469
hm sandbox vm machine snapshot download web-01 snap-61d5adc592c0469 web-01.img
```

- **Only while stopped.** Taking or restoring one while the machine runs is
  refused with 409: a disk copied under a running guest is caught
  half-written.
- **A copy that costs what differs, where it can.** The node copies with
  `cp --sparse=always --reflink=auto`: on XFS or Btrfs a snapshot shares the
  disk's blocks until they change, and elsewhere it costs what the disk
  holds, not its size. `bytesOnNode` says which.
- **A restore puts back the disk and its size**: one taken before a resize
  gives the machine its earlier size again. It goes beside the disk first
  and over it in one rename, so a restore that fails leaves the disk as it
  was. Snapshots are independent of each other and of the disk.
- **A download is the whole raw disk**, as long as the disk is; `hm`
  writes its zeros as holes. It is the image the guest boots, so `debugfs`
  or a loop mount reads it without booting anything.
- **Its files are read without booting it.**
  `GET .../snapshots/{id}/files?path=/etc` lists a directory and
  `GET .../snapshots/{id}/file?path=/etc/hostname` is one file, up to 64 MiB
  (`hm ... snapshot ls` and `cat`). An ext2, ext3 or ext4 filesystem is
  read, with e2fsprogs' `debugfs`, which changes nothing: a template
  machine's disk, or in a disk image the partition named with
  `partition=N`, else the largest ext one, a cloud image's root. Symbolic
  links are listed and not followed; XFS and Btrfs are not read.
  [Checked on KVM](benchmarks/2026-10-12/snapshot-files-kvm/README.md),
  Ubuntu's cloud image included.
- **The cloud-init seed is not in one**, and is left as it is by a restore.
- At most 32 a machine; deleting the machine deletes them.

[Checked on real KVM](benchmarks/2026-10-12/machine-snapshots-kvm/README.md).

## Resizing

A stopped machine can be given other processors, other memory, or a larger
disk. It has them the next time it starts.

```sh
curl -sS -X POST  $NODE/machines/web-01/stop
curl -sS -X PATCH $NODE/machines/web-01 -H 'content-type: application/json' \
  -d '{"cpuCount":4,"memoryMB":8192,"diskGiB":40}'
curl -sS -X POST  $NODE/machines/web-01/start

hm sandbox vm machine resize web-01 --cpus 4 --memory-mb 8192 --disk-gib 40
```

- **Any of the three, alone or together.** What is not given is left.
- **The machine is stopped.** A running one is refused with 409: nothing
  here changes a size under a running guest.
- **A disk only grows.** A smaller one is refused, since making it would
  mean choosing what on it to lose.
- **A machine from a template finds the space.** Its disk is one ext4
  filesystem the node made, and the node checks it and grows it with the
  disk.
- **A machine from an image has a longer disk** and its own partitions as
  they were. A cloud image grows its root partition itself when it boots;
  any other is grown from inside.
- **A size that cannot be is refused**, not rounded to one that can.

[Checked on real KVM](benchmarks/2026-10-11/machine-resize-kvm/README.md).

## Files

Bytes go into a machine's guest and come out of it by path.

```sh
curl -sS -X PUT "$NODE/machines/web-01/files?path=/srv/app/config.toml&user=dev" \
  --data-binary @config.toml
curl -sS "$NODE/machines/web-01/files?path=/var/log/app.log" -o app.log

hm sandbox vm machine upload web-01 config.toml /srv/app/config.toml --user dev
hm sandbox vm machine download web-01 /var/log/app.log app.log    # or `-` to print it
```

- **A write makes the directories above the file.** With `user`, the file
  and the directories it made are that guest user's; root's otherwise. A
  read is as root.
- **A path is absolute and names the file itself.**
- **A file is at most 64 MiB**, either way: it is held whole in the node's
  memory on its way through. Something larger belongs on a disk or comes
  over the machine's network.
- **A write is in place.** One that fails part way leaves what it had
  written; the file is not swapped in whole at the end.
- **It needs the guest agent**, as `exec` does.
- `download` does not write over a local file that is there.
- A refusal says whose it is: 404 for a file that is not there, 400 for a
  path or a user that cannot be, 403 where the guest forbids it, 413 over
  the limit, 409 when the machine is not running.

[Checked on real KVM](benchmarks/2026-10-11/machine-files-kvm/README.md).

## Prompts

A coding agent is a program with a command-line form: it takes one prompt,
works, prints what it does a line of JSON at a time, and exits. A prompt is
a [run](#runs) of one, with a command line the node writes and an output it
reads.

```sh
curl -sS -X POST $NODE/machines/web-01/prompts -H 'content-type: application/json' -d '{
  "provider": "claude", "prompt": "make the tests pass", "cwd": "/srv/app",
  "user": "dev", "unattended": true, "env": {"ANTHROPIC_API_KEY": "..."}}'
# → {"promptID":3,"state":"running","provider":"claude","prompt":"make the tests pass",...}

# Its events so far. `next` is where to read from the next time.
curl -sS "$NODE/machines/web-01/prompts/3?after=0"
# → {"state":"running","session":"a0945f92-...","next":4631,
#    "events":[{"offset":0,"event":{"type":"system","session_id":"a0945f92-...",...}},...]}

curl -sS -X POST $NODE/machines/web-01/prompts/3/interrupt   # what Ctrl-C would be
curl -sS $NODE/machines/web-01/prompts                       # every prompt kept, oldest first
```

With `hm`, which follows the prompt to its end, prints each event on a line
and exits with the agent's code (`--detach` to start it and not wait):

```sh
hm sandbox vm machine prompt web-01 --provider claude --user dev --unattended \
  --cwd /srv/app --pass-env ANTHROPIC_API_KEY "make the tests pass"
```

| `provider` | What is run |
|---|---|
| `claude` | `claude -p "$HM_PROMPT" --output-format stream-json --verbose` |
| `codex` | `codex exec --json --skip-git-repo-check "$HM_PROMPT"` |
| `custom` | Your `command`, a shell line that reads `$HM_PROMPT` (and `$HM_MODEL`). For any other agent |

- **`model`** is passed as the agent's own flag. **`resume`** carries on a
  session of Claude Code's: give an earlier prompt's `session`. It is refused
  for `codex`.
- **`unattended`** lets the agent act without asking, with the flag each
  calls dangerous. It is off unless given. Claude Code will not take it as
  root, so with `claude` it needs a `user`.
- **The prompt is never part of the command line.** It reaches the agent in
  the environment, so nothing in it is read by a shell.
- **`env` is where the agent's key goes.** It is not kept, returned or
  logged. `--pass-env NAME` gives the agent a variable of your own.
- **An event is a line the agent printed.** A line of JSON is given as
  `event`; any other line as `text`. Half a line is not given until the rest
  arrives. The agent's standard error is not events: it is the run's, at
  `/runs/{id}`.
- **A prompt is a run** with the same ID, so it ends, is kept and is lost as
  a run is.
- **The agent must be in the machine.** Create a machine from an image
  with `guestAgent` and `agents: ["claude", "codex"]` and its first boot
  installs them; see [below](#agents-installed-at-first-boot). Any other
  agent goes in by `userData`. A prompt for an agent that is not there ends
  with exit code 127.
- **Not here:** steering a turn that is running, and conversations that are
  named, listed or moved from one agent to another.

Each provider's line was given to the agent as published (Claude Code
2.1.296, Codex CLI 0.162.1) with no key: both took it, printed a first event
naming a session, and ended for want of one. **No prompt has been answered by
a model here.** Everything between a caller and the agent is
[checked on real KVM](benchmarks/2026-10-11/machine-prompts-kvm/README.md)
with a stand-in for the agent.
Installed at first boot, both agents were
[prompted through the API on real KVM](benchmarks/2026-10-12/machine-agents-kvm/README.md),
and each stopped at the same place, for want of a key.

### Agents installed at first boot

```sh
curl -sS -X POST $NODE/machines -H 'content-type: application/json' -d '{
  "name": "ai-01", "image": "noble.raw", "diskGiB": 8, "memoryMB": 2048,
  "guestAgent": true, "agents": ["claude", "codex"],
  "network": {"allowInternetAccess": true}}'

hm sandbox vm machine create ai-01 --image noble.raw --guest-agent   --agent claude --agent codex --network --disk-gib 8 --memory-mb 2048
```

The node adds the install to the cloud-init vendor data that installs its
guest agent, after it:

- **Node and npm** come from the distribution's own packages (`apt-get` or
  `dnf`) when the image has no `npm`, then `npm install -g` installs
  `@anthropic-ai/claude-code` and `@openai/codex`, at the newest version
  published at that moment.
- **It needs the network.** Without one, or with npm's registry not
  allowed, the install fails; the machine still boots and runs its guest
  agent.
- **`exec` answers before it finishes.** `/var/lib/hv2/agents-ready` is
  written when it is done, and `/var/log/hv2-agents.log` says what it did or
  why it failed. Wait for the file before prompting.
- **Once in the machine's life.** It is cloud-init's first boot, so a
  restart does not install them again, nor update them.
- **No key comes with them.** Each prompt brings its own in `env`.


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
- **Networking beyond egress and published ports.** A machine is reached
  through [ports the node publishes](#published-ports), in TCP, and not at
  an address of its own or by a name with TLS as a sandbox's ports are.
  There is no bridged or VLAN networking and no second NIC. Its network
  cannot be changed after creation, and it does not join private sandbox
  networks.
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
