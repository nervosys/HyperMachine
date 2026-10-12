# OTLP export, against the OpenTelemetry Collector

**What this checks:** a node [pushing its metrics over OTLP](../../../TELEMETRY.md)
to a real OpenTelemetry Collector, and the collector taking them as the
numbers they are. boat sends OTLP over HTTP, set per account; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-otlp-export.py` runs the OpenTelemetry Collector (contrib,
0.162.0, the release binary) with [this configuration](collector.yaml): an
OTLP HTTP receiver that demands a bearer token, a file exporter recording
what arrived, and a Prometheus exporter serving it again. Then it runs
`hv2-sandboxd`, with a template, pushing every second.

1. **The same numbers.** Three sandboxes are created and deleted, so the
   counters and the create histogram have something in them. Once the
   node's numbers stop changing and two more pushes have gone, every sample
   the node serves at `/metrics` must be served by the collector, with the
   same labels and the same value: each gauge, each counter's every label,
   each histogram's every bucket, sum and count. The labels the collector
   adds itself (`job`, `instance`, `otel_scope_*`) are set aside.
2. **What arrived.** In the collector's file: `service.name` and
   `service.instance.id` on the resource, a histogram whose buckets add up
   to its count, its unit `s`, and counters named without `_total`.
3. **The collector away.** It is stopped: pushes must fail and be counted.
   Started again: they must succeed again. The node's log must say each
   once.
4. **The environment.** A second node configured only by
   `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS` (the token
   percent-encoded), `OTEL_SERVICE_NAME`, `OTEL_METRIC_EXPORT_INTERVAL` and
   `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`. Its pushes must arrive under its
   service name.
5. **A wrong token.** A third node with another token: the collector must
   refuse every push, the node count them as failed and log the 401. No log
   of any node may hold either token.
6. **A wrong protocol.** A node with `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` must
   refuse to start, and say why.

## Result

All six passed on the first run, on Linux 6.18.33 under WSL2 with nested
KVM, on 2026-10-12. From [`report.json`](report.json):

- 70 samples compared, from 5 gauges, 3 counters and 4 histograms, every
  one equal. The node's [`/metrics`](node-metrics.txt) and the
  [collector's](collector-prometheus.txt) at that moment are kept.
- The collector refused the wrong token with `401 Unauthorized:
  provided authorization does not match expected scheme or token`.
- The node with `grpc` exited 1: `OTEL_EXPORTER_OTLP_PROTOCOL=grpc: metrics
  are pushed as http/json only`.

## What this does not show

- **A histogram spread over several buckets** through the collector. The
  three creates all fell in one bucket. Spreading is unit-tested
  (`metrics::tests::the_otlp_form_is_the_same_numbers_in_otlp_s_terms`), and
  not driven through a collector.
- **HTTPS**, and any collector but this one: no hosted service, no other
  receiver.
- **A long run**, or a node under load.
- **The control plane**, which does not push.
