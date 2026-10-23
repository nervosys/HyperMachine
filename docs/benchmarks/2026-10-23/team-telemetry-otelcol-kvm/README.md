# A team's telemetry into the real OpenTelemetry Collector, on KVM (2026-10-23)

`tools/check-team-telemetry-kvm.py --otelcol PATH` on WSL2's KVM, with the
**OpenTelemetry Collector contrib 0.163.2** (`otelcol-contrib`, from the
project's GitHub release; the tarball's sha256 is
`cc19dd6a1b85ca694ef16655688e597f693d0e24d2baecdd50e2db597819eeff`) as red's
destination in place of the script's Python collector. Its OTLP/HTTP receiver
requires `Authorization: Bearer red-token` (the `bearertokenauth` extension),
so a push without the team's header is refused by the collector, not by the
check; its file exporters write what it accepted. All 7 cases pass
(`report.json`, which names the collector's version):

- the collector received red's metrics with `hypermachine.team=red` and the
  right counts (sandboxes running and paused, vCPUs, memory), read back from
  the collector's own output;
- they followed the sandboxes (a resumed one is counted, usage is more than
  zero);
- the control plane's `GET /telemetry` reports the last push as `ok`, which
  for the control plane means the metrics, logs and traces posts all got a
  2xx from the collector. (The first run reported `logs: 404` because that
  collector had only a metrics pipeline; the fault was the configuration.)

Only the metrics the collector wrote were read back. NOT VERIFIED: the
logs' and traces' content as the collector parsed it (only that it accepted
them), TLS to the collector, and any collector other than this one.
