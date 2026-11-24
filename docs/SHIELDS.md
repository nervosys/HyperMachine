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

## Operating it

### The node

```
hv2-sandboxd --network --shield-dir /var/lib/hv2/shields [--shield-min-monitor 3600] [--shield-two-keys]
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
| Kernel | patched bpf-next, systemd 256 | any Linux ≥ 5.5 (seccomp user-notify); none for network |
| What it can refuse | any kernel function an eBPF program can hook | trapped syscalls (with argument tests) and network destinations |
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
- **Shields can refuse only system calls and network destinations.** They
  cannot patch an arbitrary code path the way an eBPF program on a kernel
  function can. The planned stage 2 is an eBPF backend behind the same shield
  API.
- **Pointer arguments are never read.** What a pointer refers to can change
  between the read and the call. Rules test register values only.
- **Only the trappable set can be shielded live.** A process sandbox traps
  that set from the start.
- **In a VM sandbox, the guest's system calls go to its own kernel.** The host
  does not trap them, and the guest agent does not apply syscall shields yet.
  A VM sandbox's network is shielded.
- **A connection refused by the name it sends has already been dialled**,
  though none of the guest's bytes have gone up it.
- **A name rule refuses DNS questions and the names that connections send.**
  It does not refuse an address the name resolves to: an address serves other
  names too. A connection by address alone needs a CIDR rule.
- **The hit log keeps the first 256 hits and the latest 1024.** It is not
  persisted.
- **The measured log grows with every change and is never compacted.**
