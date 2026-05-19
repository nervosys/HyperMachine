#!/usr/bin/env bash
# Workload: cold boot time.
# Special-cased by runner: invoked OUTSIDE a running VM. Runner does
# hv setup → start → wait_ssh and times the latter; this script is a no-op
# placeholder that documents the metric name.
#
# Workload contract: prints TSV lines `metric<TAB>value<TAB>unit`.
echo -e "boot_cold_seconds\t${BENCH_VALUE:-0}\ts"
