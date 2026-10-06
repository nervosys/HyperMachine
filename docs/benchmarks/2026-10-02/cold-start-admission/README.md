# Opt-in cold-start admission

The implemented `--cold-start-concurrency` budget remains disabled by default.
The final current-source binary passed 39 Linux and 35 Windows daemon tests,
strict Linux Clippy with the existing `too_many_arguments` exception, and the
live admission/failure-release/snapshot-resume fixture.
The final binary also passed all 23 KVM/TLS lifecycle, SSH, authorization and
named-creation regression cases, with 222 authenticated audit records, no
unfinished admissions, empty guest inventory and all 22 owned processes stopped.

Fresh-daemon AB/BA cohorts use eight available CPUs, one vCPU and 1024 MiB per
guest, the same kernel/initrd, no added CPU worker and an unchanged 15-second
guest readiness deadline. Readiness includes queueing, creation and command
validation. Each guest is retained until batch validation and then deleted.
All attempts and unsuccessful fixtures are archived.

| Cohort, C100 | Uncapped pass/attempt | Limit eight pass/attempt | Uncapped P50 / P99 ms | Capped P50 / P99 ms | Faster complete paired means |
|---|---:|---:|---:|---:|---:|
| Prototype first | 400/400 | 400/400 | 6841 / 11497 | 3854 / 9025 | 4/4 |
| Prototype repeat | 304/400 | 400/400 | 7042 / 16809 | 4147 / 8102 | 3/3; one incomplete pair retained |
| Final current source | 400/400 | 400/400 | 6702 / 7898 | 3639 / 8434 | 4/4 |

Final P50 decreased 45.7%; median paired mean reduction was 3201 ms.
**Final P99 increased 6.8%.** Conditional latency excludes unsuccessful attempts;
the prototype repeat's 96 creation failures remain in totals and raw records.
This supports an optional workload tradeoff, not superiority across percentiles,
a universal optimal budget or a fix for the underlying timeout cause.

Default-disabled controls compare the previous accepted daemon to the full
current binary, including intervening committed changes. C1 passed 8/8 attempts
and C8 passed 64/64. C1 P50 was 378.96 versus 378.73 ms; C8 was 499.60 versus
513.15 ms (current 2.7% slower), with only one of four C8 paired means faster.
These small shared-host samples establish functional regression coverage and
do not establish performance equivalence or isolate this patch's overhead.

The live fixture records eight acquisitions/releases with a maximum of two,
and two acquisitions/releases with a maximum of one during failure recovery.
Snapshot resume completes while that single cold slot remains occupied.
Earlier unsuccessful fixture runs preserve parser and fixture setup errors;
their frozen checker versions accompany the logs.

`adoption/compiled-main.rs` is the exact full-current compiled overlay on
`f53adee`. The three provisional boot changes were excluded. `prototype` uses
the earlier clean registration source described in its build context. Raw
analysis retains `runtime_change_adopted:false` as an experiment designation;
this change adopts only the optional control, leaving defaults unchanged.

Run `python -O tools/verify-cold-start-admission.py` with this directory to
verify file identities, recomputed cohorts, all planned attempts, cleanup and
live admission bounds. Frozen coordinators reproduce runs with explicit owned
binary/image paths. No executable, private key or operator credential is archived.
No managed competitor endpoint was available and no competitor win is established.
