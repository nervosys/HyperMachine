# The warm pool

Most of what a create waits for is the same for every sandbox of a template:
restore the template's guest, then wait for its agent to answer and take a
fresh clock and random seed. None of that depends on who is asking.

With `--warm-pool N`, a node keeps N guests that have already done it. A create
takes one and gives it what is its own. On the host this was measured on, that
took a create from about 19 ms to under 1 ms.

```sh
hv2-sandboxd --warm-pool 8 ...
curl -s localhost:3980/pool      # {"target":8,"ready":8,"handedOut":0,"missed":0}
```

## How it works

- **A spare** is a guest restored from the base template whose agent has
  answered and been given its own clock and random seed. It is then put in
  [standby](STANDBY.md), where it uses no CPU.
- **A create takes one,** wakes it, and gives it its ID, its access token, its
  network, its volume mounts and its environment. Another spare is restored in
  the background.
- **An empty pool is not an error.** The create restores a guest as it would
  without a pool. `missed` counts how often that happened, which is how to tell
  that the pool is too small for the rate of creates.

`GET /pool` reports the target, how many spares are ready, how many creates took
one (`handedOut`) and how many found the pool empty (`missed`).

## What to know

- **Spares cost memory, over and above `--capacity`.** A node with a pool of N
  can hold N more guests than it admits. Size the pool for the burst you want
  served instantly, not for throughput: a refill takes as long as a create did.
- **Only the base template has spares,** and only a plain create takes one. A
  create from another template or a snapshot, a resume, a fork, and a sandbox
  with a [disk](DISKS.md) all take the path they did before.
- **Sandboxes from the pool share no random state.** Each spare is restored and
  reseeded on its own, as each sandbox created without the pool is.
- **A spare is not a sandbox** until a create takes it: it is not listed, has no
  ID and no token, and nothing can reach it.
- **A node that is restarted** starts with an empty pool and fills it once its
  template is ready.

Evidence: [real KVM: the pool's behaviour, and create times with and without it](benchmarks/2026-10-08/warm-pool-kvm/README.md).
