# Telemetry from a node

A sandbox node (`hv2-sandboxd`) gives its metrics two ways: as Prometheus text
at `/metrics` for a scraper that can reach it, and pushed over OTLP to an
OpenTelemetry collector its operator names. Both are the same numbers, built
once per scrape or push.

## What is measured

| Metric | Kind | What |
|---|---|---|
| `hv2_node_sandboxes_running` | gauge | Sandboxes running on the node |
| `hv2_node_sandboxes_paused` | gauge | Sandboxes suspended to its disk |
| `hv2_node_sandboxes_booting` | gauge | Creations in flight |
| `hv2_node_capacity` | gauge | Sandboxes it will run at once |
| `hv2_node_template` | gauge | Templates sandboxes are restored from |
| `hv2_node_creates_total{result}` | counter | Creations: `ok`, `full`, `rejected`, `error` |
| `hv2_node_sandbox_ends_total{reason}` | counter | Sandboxes ended: `deleted`, `expired` |
| `hv2_node_transitions_total{kind}` | counter | Pauses, resumes, evictions, forks, reboots |
| `hv2_node_create_seconds` | histogram | Request to an answering agent |
| `hv2_node_pause_seconds` | histogram | Suspending a sandbox to disk |
| `hv2_node_resume_seconds` | histogram | Resume request to an answering agent |
| `hv2_node_checkpoint_seconds` | histogram | A fork's source paused for its checkpoint |
| `hv2_node_otlp_pushes_total{result}` | counter | Pushes over OTLP, `ok` or `failed`; only when pushing |

`/metrics` needs no token: it says how many, never whose.

**Not measured here:** anything per sandbox or per machine, or per API key.
Machines are not counted at all yet. A node's numbers are the node's; a
team's own are [pushed by the control plane](#a-teams-own-telemetry).

## Pushing over OTLP

```sh
hv2-sandboxd ... \
  --otlp-endpoint https://collector.example:4318/v1/metrics \
  --otlp-header "Authorization=Bearer $COLLECTOR_TOKEN" \
  --otlp-interval 15
```

Or with OpenTelemetry's own variables, which the flags override:

| | Flag | Variable |
|---|---|---|
| Where | `--otlp-endpoint URL`, the whole URL | `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT`, as it is; or `OTEL_EXPORTER_OTLP_ENDPOINT`, with `/v1/metrics` added |
| Headers | `--otlp-header KEY=VALUE`, repeated | `OTEL_EXPORTER_OTLP_HEADERS` (or `..._METRICS_HEADERS`): `KEY=VALUE,KEY=VALUE`, values percent-encoded |
| How often | `--otlp-interval SECONDS`; 15 unless said, 1 at least | `OTEL_METRIC_EXPORT_INTERVAL`, milliseconds |
| Service | | `OTEL_SERVICE_NAME`; `hv2-sandboxd` unless said |

- **OTLP/HTTP with JSON only.** An `OTEL_EXPORTER_OTLP_PROTOCOL` of anything
  else (`grpc`, `http/protobuf`) stops the node at start, rather than sending
  JSON to a port that expects gRPC. A collector's OTLP HTTP receiver takes
  JSON; its gRPC port, 4317 by convention, does not.
- **Each push is everything, cumulative.** Counters and histograms count from
  when the node started, so a push that fails loses nothing: the next one
  carries it. A failure is logged when pushes start failing and when they
  work again, not every time, and counted in
  `hv2_node_otlp_pushes_total{result="failed"}`.
- **The resource** is `service.name` and `service.instance.id`, the node's
  cluster ID (`local` when it is not in a cluster).
- **Names are OpenTelemetry's.** A counter loses Prometheus's `_total` and a
  histogram its `_seconds`, with its unit `s`; a collector exporting to
  Prometheus puts both back, so the names there are the ones in the table.
- **Header values are not logged**, nor shown when the configuration is.
  Credentials in the URL are refused, since the URL is logged.
- **Only metrics.** No logs and no traces are pushed.

[Checked against the OpenTelemetry Collector](benchmarks/2026-10-12/otlp-export/README.md):
what it then served as Prometheus was, sample for sample, what the node
served at `/metrics`.

## A team's own telemetry

A team can have the control plane push *its* numbers to a collector it names,
which a node's operator-set push above cannot do:

```sh
# The operator, once: the collectors teams may use.
hv2-control-plane ... --team-otlp-endpoint https://collector.example/teams

# A team (a key with the `telemetry` scope, or `admin`):
curl -X PUT $CONTROL/telemetry -H "x-api-key: $KEY" -H 'content-type: application/json' -d '{
  "endpoint": "https://collector.example/teams/red/v1/metrics",
  "headers": {"Authorization": "Bearer ..."}, "intervalSeconds": 60}'
curl $CONTROL/telemetry -H "x-api-key: $KEY"      # where, header names, how the last push went
curl -X DELETE $CONTROL/telemetry -H "x-api-key: $KEY"
```

| Metric | What |
|---|---|
| `hm_team_sandboxes_running` | The team's sandboxes running now |
| `hm_team_sandboxes_paused` | The team's sandboxes paused |
| `hm_team_vcpus_running` | vCPUs they run with |
| `hm_team_memory_mib_running` | MiB they run with |
| `hm_team_running_seconds_24h` | Running time in the last 24 hours |
| `hm_team_vcpu_seconds_24h` | vCPU-seconds in the last 24 hours |
| `hm_team_memory_gib_seconds_24h` | GiB-seconds in the last 24 hours |

All are gauges, sent as a resource with `service.name` `hypermachine`,
`hypermachine.team` and `service.instance.id`. The usage figures are a
rolling day, not a running total: a total would mean reading the whole
[usage ledger](USAGE.md), which is never trimmed, on every push.

- **The operator decides where.** The control plane makes the request, from
  inside the operator's network, to an address a team chose. So a
  destination is accepted only under a `--team-otlp-endpoint` prefix (the
  same scheme, host and port, and a path at or below it), and with none
  given a team cannot set one (403) and nothing is sent. It is checked when
  stored and again at each push, so removing a prefix stops what was stored
  under it. **No redirect is followed.**
- **No credentials in the URL, no query.** A key goes in a header: up to 8,
  not the ones that frame a request. Headers are kept as the rest of the
  store is, **unencrypted**, and are never returned or logged; `GET` shows
  their names.
- **One destination a team**, 15 seconds to an hour between pushes (60
  unless said). A push is sent within 5 seconds of a change, and failures
  are logged when they start and stop, not each time. `GET /telemetry` says
  how the last one went.
- **Each team is sent its own and nobody else's.** A key without a team
  (the operator's) has a destination of its own, for the sandboxes without
  one, labelled `-`.
- **Every control plane given the flag pushes.** With several, a collector
  is sent each team once per instance, told apart by `service.instance.id`.

[Checked on real KVM](benchmarks/2026-10-14/team-telemetry-kvm/README.md).

## Not here

- **A node's numbers per team.** The control plane pushes a team's own;
  `/metrics` on the control plane is still its own, and serves no team's.
- **The `hv2` server and `hv2-core`'s exporter.** `hv2-core` has an OTLP
  exporter of its own behind its `remote-telemetry` feature, which no
  shipped binary turns on. `hv2-core`'s `[observability]` table is
  parsed by nothing either, so `config.example.toml` no longer has one. What is described here is
  the node's.
