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

**Not measured:** anything per sandbox or per machine, per API key or per
team. Machines are not counted at all yet.

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

## Not here

- **The control plane** serves `/metrics` and does not push.
- **The `hv2` server and `hv2-core`'s exporter.** `hv2-core` has an OTLP
  exporter of its own behind its `remote-telemetry` feature, which no
  shipped binary turns on, and `config.example.toml`'s `[observability]`
  settings are read by nothing. What is described here is the node's.
