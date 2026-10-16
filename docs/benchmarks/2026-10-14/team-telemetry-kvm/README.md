# A team's own telemetry

**What this checks:** that the control plane pushes each team's own numbers
to the collector that team set, and nobody else's. Before this only a
node's operator could send metrics anywhere. See
[how it works](../../../TELEMETRY.md#a-teams-own-telemetry) and the
[comparison](../../../BOAT_PARITY.md).

## Method

`tools/check-team-telemetry-kvm.py` runs an owned Redis, one `hv2-sandboxd`
node and the `hv2-control-plane`, over verified TLS and node mTLS, with a
private CA the check makes. Two collectors run on loopback inside the check
and keep what they are POSTed. The control plane is started with both
collectors as `--team-otlp-endpoint` and no other. Three API keys: red and
blue, each with the `telemetry` scope, and a third red key without it.

1. **Refusals.**
   - A team with nothing set gets 404.
   - A destination under no prefix the operator listed gets 403.
   - A query in the URL, credentials in the URL, a reserved header and an
     interval of 1 second each get 400.
   - A key without the `telemetry` scope gets 403, on both `GET` and `PUT`.
2. **Sandboxes.** Red creates two sandboxes and pauses one, through the
   control plane. Blue creates one.
3. **Set and pushed.** Red sets collector A with `Authorization: Bearer
   red-token`, blue sets collector B with `X-Team-Key: blue-token`, both
   every 15 seconds. Both collectors are pushed to.
4. **Each its own.**
   - Red's collector: resource attribute `hypermachine.team` = `red`; 1
     sandbox running and 1 paused; 1 vCPU and 256 MiB running; header
     `Bearer red-token`.
   - Blue's: `blue`; 1 running, 0 paused; header `blue-token`.
   - Neither collector, in any push it ever got, has the other team's name
     or key in it.
5. **Following the sandboxes.** Red resumes the paused sandbox, asking for
   a 30-minute life. The next push counts 2 running and 0 paused, and
   usage over the last 24 hours is more than zero.
6. **Read back.** `GET /telemetry` shows each team its own endpoint, its
   header *names*, and how the last push went (`ok`), and no header value.
7. **Deleted.** Red deletes its destination (204; 404 for the next delete
   or get). Over the next 40 seconds red's collector gets nothing more and
   blue's goes on getting pushes.

## Result

All seven cases passed in each of five consecutive runs on 2026-10-16, on
Linux 6.18.33 under WSL2 with nested KVM. From [`report.json`](report.json),
the last run: red's collector was sent 1 running and 1 paused, then 2
running and 0 paused once red resumed, with 43.6 running seconds in the last
24 hours; blue's collector got two more pushes while red's got none.

In unit tests (193 in `hv2-cluster`, with a real Redis for the store ones),
a destination removed from the operator's list is not sent to, a redirect is
not followed, and two teams each get only their own collector.

## A flaw in the first version of this check

The first version resumed red's sandbox without a timeout and failed 3 runs
in 5, at the "following" step, with red at 1 running and 0 paused. The cause
was not the telemetry. **The resume route gives a sandbox 15 seconds to
live unless told otherwise**, as E2B's does. A sandbox resumed that way
expired, its record left the store, and the control plane reported what the
store held. Only a push that landed within those 15 seconds saw 2. The check
now resumes with `"timeout": 1800`, and the failing runs' store dump (the
set of IDs and each record) is what showed two records, not three.

## What this does not show

- **A control plane given the flag on several instances.** Each pushes; a
  collector would see each team once per instance. Not run.
- **A destination removed from the list on a live control plane.** The list
  is set by a flag at start, so this is covered by a unit test and not by
  this run.
- **HTTPS collectors.** The collectors here are plain HTTP on loopback.
- **Large teams.** A push reads every sandbox record once per round.
