# Shields

A shield is a rule that refuses something a running sandbox might do: a system
call, or a network destination. An operator adds it to sandboxes that are
already running and removes it again, with no restart. It exists to close a
hole quickly, such as a kernel advisory reachable through `io_uring` or a
command-and-control domain, and to keep it closed until a real fix ships.
After that it expires.

Every shield is:

- **signed**: ML-DSA-87 (FIPS 204), from IronCrypto. A node loads only a shield
  signed by a key in its keyring.
- **approved**: a shield loads in *monitor* mode, which records what it
  matches and refuses nothing. A separate signed *directive* enforces it. A
  node can require a minimum monitoring period and a second key.
- **expiring**: every shield names its expiry, and none may stay in force for
  more than 30 days from when it was loaded.
- **measured**: each load, mode change, removal, expiry and revocation is an
  event in a hash-chained log. The node signs the log's register over a
  verifier's nonce.

The crate is `hv2-shield`. A node's shields are served by `hv2-sandboxd` under
`/shields`, and the operator's tool is `hm shield`.

## What a shield looks like

```json
{
  "name": "cve-2026-xxxx-io-uring",
  "description": "io_uring_setup reaches the vulnerable path; refuse it until the kernel is patched",
  "advisory": "https://example.invalid/CVE-2026-XXXX",
  "scope": { "kind": "all" },
  "syscalls": [
    { "syscall": "io_uring_setup", "action": { "kind": "errno", "errno": 1 } },
    { "syscall": "unshare",
      "args": [{ "index": 0, "op": "anyBits", "value": 268435456 }],
      "action": { "kind": "errno", "errno": 1 } }
  ],
  "network": [
    { "name": "*.c2.example" },
    { "cidr": "203.0.113.0/24", "ports": { "from": 443, "to": 443 }, "protocol": "tcp" }
  ],
  "hooks": [{ "hook": "sb_mount", "errno": 1 }],
  "expiresAt": 1798761600
}
```

- `scope` is `all`, or `{"kind": "sandboxes", "ids": [...]}`.
- A syscall rule matches a trapped call by its name and, optionally, by
  register-value tests on its arguments: `eq`, `ne`, `lt`, `le`, `gt`, `ge`,
  `anyBits` and `allBits`, each under an optional mask. Its action is `errno`
  (1 to 4095) or `kill`.
- A network rule matches by CIDR, by name (`host` or `*.domain`), by port
  range and by protocol. A rule with several fields matches only when all of
  them match.
- A hook rule names a Linux security (LSM) hook, one of
  `hv2_shield_bpf::HOOKS` such as `sb_mount` or `socket_create`, and an
  `errno` (1 to 4095). It refuses that kernel operation whichever system call
  leads to it, and tests no arguments. See "Kernel hooks" below.
- The JSON is checked strictly. An unknown field, a bad name, an argument
  index above 5, or more than 64 rules is refused. The digest that names a
  shield is the SHA-256 of these exact bytes, which are also what is signed.

## How it is enforced

### System calls, in process sandboxes

`hv2-sandbox`'s Linux backend has a new control, `Control::SyscallShield`.
When a `SandboxSpec` carries a `SyscallShield`, the child installs a seccomp
filter as the very last step before `exec`, and it does so with
no-new-privileges set. The filter returns `SECCOMP_RET_USER_NOTIF` for the
calls named, and the kernel hands its listener to the parent over a socket.
A supervisor thread in the parent answers each trapped call by asking a
`SyscallGate`. Because the gate is asked at the moment of the call, a rule
loaded later applies to the workload already running.

`hv2_shield::live::SandboxGate` (feature `sandbox`) is the gate that asks a
node's registry. `SandboxGate::shield` traps every call this host can trap,
so a shield loaded later always finds its call trapped.

Only these calls can be trapped: 56 calls that are rarely made and that
advisories name. They include `ptrace`, `bpf`, the `io_uring_*` calls,
`unshare`, `setns`, `mount`, `keyctl`, `userfaultfd`, `perf_event_open`,
`process_vm_*` and `memfd_create`. See `hv2_sandbox::trappable_syscalls()`.
A call that is not trapped never leaves the kernel, so no shield can refuse
it.

The trapped set is fixed when the workload starts. Which calls are refused is
not fixed.

### System calls, in VM sandboxes (`--shield-guests`)

A VM sandbox's guest makes its calls to its own kernel. On a node run with
`--shield-guests`, the node sends the syscall rules in force for each sandbox
and machine to its guest agent (`hv2-guest-agentd`), as soon as the guest
answers, at once when a shield changes, and every 5 seconds. From the first
list on, the agent installs the same seccomp user-notify filter on every
program it starts (`exec`, `start`, and so every command an SDK runs) and
answers trapped calls from those rules. The node collects what they matched
into `GET /shields/hits` on the same 5-second sweep, checking each report
against its own registry: a hit for a shield not in force for that sandbox is
counted as `lost`, and a guest cannot report a monitored shield as refusing.
`GET /shields` lists each guest under `syscallGuests` as `applied`,
`unsupported` (with the reason) or `unreachable`.

It is off by default because every trapped call in a guest then costs a round
trip to the agent, whatever the rules say. Its limits:

- The filter is installed as root without no-new-privileges, so `sudo` and
  setuid programs keep working; the agent must be root.
- Only programs the agent starts are trapped, with their children. A program
  started before the first list arrived, or by anything else in the guest
  (init, a daemon it launched itself), is not.
- Root in the guest can kill the agent. Its trapped calls then fail with
  `ENOSYS` until it is restarted, and programs started after that are not
  trapped. A guest shield stops an unprivileged process; it is not a barrier
  against guest root.
- `io_uring_enter`, `splice`, `tee` and `vmsplice` are trapped only in
  programs started while a rule names them.
- In a guest, `HV2_SHIELDS=off` in the agent's environment turns it off, and
  `HV2_SHIELD_SKIP=a,b` leaves more calls untrapped. Both are the guest
  image's to set.
- An agent from before this release does not know the operation; its sandbox
  is listed as `unsupported`, and it still starts.

### Network, in VM sandboxes

Every sandbox's gateway (`hv2-net`) consults a `ConnectionShield` after its
own policy allows something. It does so at four points:

- each DNS question, so a refused name does not resolve;
- each TCP SYN, by address, before anything is dialled;
- the name the connection sends (TLS SNI or HTTP Host), checked before any of
  the guest's bytes go upstream, including a name that is sent late;
- each UDP session to a private network.

When a shield has a rule about names, connections are read for their name
before they are relayed. A connection that starts naming something and never
finishes is refused. When no shield in force for a sandbox has a name rule,
connections are not read. `hv2-sandboxd` installs the shield on every
sandbox's and machine's gateway when it starts, and the gateway asks the
node's registry live.

### Kernel hooks (eBPF, `hooks` rules)

A `hooks` rule refuses a kernel operation at a Linux security (LSM) hook, such
as `socket_create`, `sb_mount` or `ptrace_traceme`, whichever system call
leads there. It is a BPF LSM program, and the engine is `hv2-shield-bpf`:

- The programs are assembled by hand in that crate, one per hook, with no BPF
  compiler or libbpf. `hv2_shield_bpf::HOOKS` lists the hooks a shield may
  name. Each returns `int`, and none needs its arguments read, so a hook rule
  tests nothing but the hook. Its action is an `errno` (1 to 4095). A hook
  cannot kill.
- A program is attached only while some rule names its hook, and detached when
  the last one is removed. A node with no hook rules loads nothing.
- Hook rules are enforced only in VM guests, with `--shield-guests`, for now.
  The engine can scope a rule to one cgroup, but nothing on a node yet applies
  hook rules to the host's own process sandboxes. In a guest the agent sets the rules for the whole guest, and the agent's own process is
  the one thing never judged, so it can keep talking to the host under a rule
  on `socket_create`. Unlike a guest's syscall rules, a hook rule therefore
  covers processes the agent did not start (init, a console shell, a daemon
  already running), and it is in force as soon as the guest has it.
- The kernel counts; nothing records each call. A hook may be asked
  millions of times a second. The engine keeps two counters per hook,
  matches while monitored and refusals while enforced, and the node reads
  them. A hit in `GET /shields/hits` for a hook is one entry for the matches
  since the last read, and adds that many to the shield's tally. It does not
  say which process or when. When several shields name one hook, the first
  enforced one decides (its `errno` is returned) and the others record the
  matches as not enforced.
- The node pushes a guest's hook rules by their own operation, `SetHookShields`,
  with their own status in `syscallGuests` (`"hooks": {"status": ...}` beside
  the syscall status). An empty list is not sent to a guest that was never sent
  one. The agent also drops a rule on its own at its expiry, on the guest's
  clock, if the host has stopped talking.

What it needs, and where it stops:

- A kernel with `bpf` in its active LSM list (the `lsm=` boot parameter),
  and BTF in `/sys/kernel/btf/vmlinux`, and a root caller. Without them the
  rule is not applied, and the guest is listed as `unsupported` with the
  reason. The stock guest kernel configuration
  (`tools/guest-image/hv2-guest.defconfig`) has neither, so guests built from
  it answer unsupported until the image enables both:
  `tools/guest-image/build-kernel.sh --shields` appends `shields.fragment`,
  which turns on both (building BTF needs `pahole` on the build host). That
  kernel build has not yet been booted with a hook rule in force; the engine
  is tested on a WSL2 kernel only. An agent from before
  hook shields is also `unsupported`; its sandbox still starts.
- A hook is coarse. The operation is refused for everything in scope, so a
  rule on `sb_mount` or `socket_create` also refuses it to the guest's own
  init and services.
- Root in the guest can unload it by killing the agent: the programs are
  attached by links the agent holds, and they go when it does. A hook shield
  is not a barrier against guest root.
- There are no per-argument tests, and no cgroup scope inside a guest.

## Operating it

### The node

```
hv2-sandboxd --network --shield-dir /var/lib/hv2/shields [--shield-min-monitor 3600] [--shield-two-keys] [--shield-guests]
```

The shield directory holds three files:

- `keyring.json`: the trusted signers, revoked signers and revoked shields.
  The operator writes this file, and `POST /shields/keyring` makes the node
  read it again.
- `node.key`: the node's attestation key, created on first start with mode
  0600.
- `registry.json`: the shields and their log, rewritten after every change.

On start, the node checks `registry.json` again: the log must replay to its
register, and each entry must be the shield its digest names. Each entry's
state and load time must also match what the log says, and its signature must
still verify against the keyring. Shields whose signature no longer verifies
are retired, and the retirement is measured. If the file was edited, the node
refuses to start until an operator moves it away.

Without `--shield-dir`, shields are kept in memory with an empty keyring, so
nothing can be loaded.

### The operator

```
hm shield keygen --out alice.key            # prints the public key for keyrings
hm shield check io-uring.json               # validates, prints the digest
hm shield propose io-uring.json --by agent  # anyone may propose; it does nothing
hm shield load --key alice.key io-uring.json   # signed, loaded in monitor mode
hm shield hits                              # what it matched while monitored
hm shield enforce --key bob.key <digest>    # a signed directive; with --shield-two-keys,
                                            # bob's key must not be alice's
hm shield monitor --key bob.key <digest>
hm shield remove  --key alice.key <digest>
hm shield attest --node-key <hex>           # fresh nonce, signature, replay: checked
hm shield sign / direct / apply             # sign offline, send from elsewhere
```

A directive names its shield by the full 64-digit digest and is valid for ten
minutes after it is issued. A node refuses a directive it has already
applied, and remembers it across restarts.

### Routes

All routes sit behind the node's cluster token. A request that carries a team
header is refused, because shields cover every tenant's sandboxes.

| Route | |
| --- | --- |
| `GET /shields` | shields, node key, register, policy |
| `POST /shields` | load a `SignedShield` (monitor mode) |
| `POST /shields/proposals` | `{"spec": "<json>", "proposedBy": "..."}` |
| `DELETE /shields/proposals/{digest}` | withdraw a proposal |
| `POST /shields/directives` | apply a `SignedDirective` |
| `POST /shields/keyring` | read the keyring file again |
| `GET /shields/hits` | matches: refused (enforced) or recorded (monitored) |
| `GET /shields/events` | the measured log |
| `POST /shields/attestation` | `{"nonce": "<hex, 16-64 bytes>"}` |

## Compared with Shirudo

[Shirudo](https://openai.com/index/introducing-shirudo/) loads signed eBPF
"upatches" into a patched kernel, measures them into TPM PCR 9, and is driven
by a CLI.

| | Shirudo | HyperMachine shields |
| --- | --- | --- |
| Signature | ML-DSA-87 | ML-DSA-87 (IronCrypto) |
| Kernel | patched bpf-next, systemd 256 | syscall and network rules: any Linux ≥ 5.5 (seccomp user-notify), host or guest, none for network. Hook rules: a kernel with `bpf` in its active LSMs and BTF; the stock guest kernel has neither |
| What it can refuse | any kernel function an eBPF program can hook | trapped syscalls (with argument tests), network destinations, and a fixed list of LSM hooks (no argument tests, counted rather than recorded per call); not an arbitrary kernel function |
| Monitor before enforce | no | yes, with a minimum period |
| Approval | no | signed directives; optional second key |
| Expiry | none ("any artifact ever signed loads at any time") | required, at most 30 days |
| Revocation | signed unload | keyring revokes signers and shields; the node retires them, measured |
| Replay of an old instruction | unload is bound to the payload | directives expire after 10 minutes and are refused if seen again |
| Measurement | TPM NV index in PCR 9, event log | hash-chained log, register signed by the node key |
| Attestation | `shirudo attest` | `hm shield attest`: nonce, signature and replay are checked |
| Interface | CLI | CLI, HTTP API, agents can propose |
| Scope | the host | per sandbox, or all |
| Network rules | no | yes |

## Limits, stated plainly

- **The node's attestation key is a file, not a TPM.** Someone who controls
  the host can sign anything. The attestation shows what the node says was in
  force, and that the log is consistent. It does not show that the host was
  honest. TPM quotes are not implemented.
- **Shields can refuse only system calls, network destinations and a fixed
  list of LSM hooks.** They cannot patch an arbitrary code path the way an
  eBPF program on any kernel function can. The hook rules are the first part
  of the planned eBPF backend; they are counted, not recorded per call, and
  test no arguments.
- **Pointer arguments are never read.** What a pointer refers to can change
  between the read and the call. Rules test register values only.
- **Only the trappable set can be shielded live.** A process sandbox traps
  that set from the start.
- **In a VM sandbox, the guest's system calls go to its own kernel.** The host
  cannot trap them. With `--shield-guests` the guest agent traps them in the
  programs it starts (see below); it is a weaker guarantee than a process
  sandbox's. A VM sandbox's network is always shielded.
- **A connection refused by the name it sends has already been dialled**,
  though none of the guest's bytes have gone up it.
- **A name rule refuses DNS questions and the names that connections send.**
  It does not refuse an address the name resolves to: an address serves other
  names too. A connection by address alone needs a CIDR rule.
- **The hit log keeps the first 256 hits and the latest 1024.** It is not
  persisted.
- **The measured log grows with every change and is never compacted.**
