# A guest's processes

Every process in a sandbox's or a machine's guest can be listed from
outside, as `ps` would list them inside:

```sh
curl -s localhost:3980/sandboxes/$SBX/processes
curl -s localhost:3980/machines/web-01/processes?all=true   # kernel threads too
# → {"processes":[{"pid":1,"ppid":0,"uid":0,"state":"S","command":"init",
#     "args":"/bin/sh /init","threads":1,"rssKiB":1240,"cpuSeconds":0.03,
#     "startedSecondsAgo":812.4}, …]}

hm sandbox vm ps $SBX [--all]
hm sandbox vm machine ps web-01 [--all]
```

```python
machine.processes()            # Python SDK
await machine.processes()      // TypeScript SDK
```

Each process:

| Field | |
|---|---|
| `pid`, `ppid` | Its ID and its parent's, as the guest's kernel numbers them |
| `uid` | Its real user ID |
| `state` | `R` running, `S` sleeping, `D` waiting on I/O, `Z` a zombie, `T` stopped, and the rest, as `ps` shows them |
| `command` | Its name, as the kernel keeps it (at most 15 characters) |
| `args` | Its command line, with spaces between the arguments; empty for a zombie or a kernel thread |
| `threads` | How many threads it has |
| `rssKiB` | Its memory resident now |
| `cpuSeconds` | User and system time it has had so far |
| `startedSecondsAgo` | How long ago it started |

- **Every process,** not only those started through the API: those a shell
  in the guest started, a service, a zombie. Before this, only processes
  started through the API and still running could be listed.
- **Read from the guest's `/proc`** by one shell through its guest agent.
  Only `/proc` and a POSIX shell are used, so a busybox guest lists as an
  Ubuntu one does. It needs the guest agent: a machine from an image without
  `guestAgent` is refused (409).
- **Kernel threads** (PID 2, and everything it started) are left out unless
  `?all=true`. **The listing itself is never in it:** the shell that reads
  `/proc`, and what it runs.
- **A moment, not a snapshot.** Each process is read in turn. One that ends
  while the list is read is left out, and one that starts may be missed.
- At most 4,096 processes, lowest PID first.
- Refused: a paused sandbox, which runs nothing (409); a stopped machine
  (409); one that is not there (404).

Evidence: [real KVM checks](benchmarks/2026-10-13/guest-processes-kvm/README.md).
