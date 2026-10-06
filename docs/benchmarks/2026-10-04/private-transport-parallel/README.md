# Concurrent private receiving authorization reads

The receiver now runs its atomic source/destination membership snapshot and two independent live-node lookups concurrently. Both authorization checks around guest port opening, actual local guest identity/pending/expiry checks, generation/owner/tag validation, the three-second deadline and active-stream revocation remain. No authorization result is cached. Invalid routes may start the bounded node reads before their snapshot is refused; the existing 128-connection setup budget still applies.

**64 daemon tests pass**, with two explicit KVM-only tests excluded from that ordinary suite. Two matching-profile dev candidate cohorts each pass **30 KVM checks and 128/128 scored operations**, plus eight warmups, with zero guests remaining and all processes reaped. Checks include owner/tag/generation refusals, actual guest DNS/binary echo, source pause/resume, stale addresses, host stream closures and target-membership revocation of a long-lived source guest stream.

| Cohort | Payload | Private setup P50 ms | Standard setup P50 ms | Difference of medians ms |
|---|---:|---:|---:|---:|
| Before 1 | 64 B | 5.163 | 3.626 | 1.536 |
| Before 1 | 1 MiB | 6.097 | 4.274 | 1.822 |
| Before 2 | 64 B | 4.576 | 3.439 | 1.137 |
| Before 2 | 1 MiB | 5.747 | 4.140 | 1.607 |
| After 1 | 64 B | 4.183 | 3.258 | 0.925 |
| After 1 | 1 MiB | 5.306 | 4.147 | 1.159 |
| After 2 | 64 B | 4.453 | 3.414 | 1.040 |
| After 2 | 1 MiB | 5.389 | 4.112 | 1.278 |

Both candidate cohorts show a smaller private-minus-standard median setup gap than either earlier cohort. This is consistent with reduced setup overhead from concurrent reads. These are separate non-interleaved before/after cohorts on a shared host; the difference of medians is not the median of paired differences, and this does not isolate a causal CPU cost or establish statistical significance. Private setup remains slower than standard. Echo throughput remains close within each candidate cohort, around 10.6–10.8 payload MiB/s at one MiB. No across-the-board or competitor win is claimed.

The archived baseline build log confirms the baseline binary uses the unoptimized dev profile. The first candidate was inadvertently built in release mode; `report-release.json` and its build/driver/logs are preserved as successful functional evidence but **excluded from before/after optimization comparisons**. Its faster unchanged standard path and lower whole-daemon PSS must not be attributed to this source change. Earlier archive prose calling the baseline a release should be read in light of this profile correction. Dev candidate SHA256: `d7c2d34157cde7268a6f9e1cd71dd0f32c6a05dbec4bc3c6e32a8c19de0f4e2e`; release candidate: `304a2f4a207e17d10e4debf1dadefd538af0b82157ffe9ed55b73d902044e21e`.

Methodology is unchanged: same host mTLS client and target KVM guest, fresh TCP/TLS, alternating paired order, concurrency one, 32 scored operations per path/payload after two warmups. Nearest-rank P50/P95; payload echo throughput divides payload MiB by complete echo time, with the payload sent each direction. Source guest gateway performance is not timed. Whole-daemon held PSS is not incremental memory; daemon CPU includes vCPU/background activity and Redis/daemon counters have 10-ms tick granularity. Raw rows, resource counters, environment and cleanup remain in each report. Recompute each with `python3 verify-results.py report-dev-v1.json` (or the v2/release name). `comparison.json` retains unrounded per-cohort values.

The only changed accepted production source is `forwards.rs`, preserved here. All 138 permitted root/isolate pairs and accepted isolated core hashes were checked; protected root core files were neither read nor built. Build with `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo build --locked --manifest-path /var/tmp/hm-egress-log-mA2CCL/Cargo.toml -p hv2-sandboxd --bin hv2-sandboxd` for matching dev mode; release adds `--release`. Drivers retain frozen input paths; reproduction elsewhere requires equivalent owned inputs and fresh output paths. Independent hosts, high concurrency, guest-origin performance, crash durability and full product parity remain unverified or incomplete.
