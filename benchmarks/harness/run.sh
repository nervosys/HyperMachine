#!/usr/bin/env bash
# benchmarks/harness/run.sh — Linux orchestrator.
#
# For each enabled hypervisor × workload in --config, run --samples + warmup,
# and write per-sample raw output plus a summary CSV under --out.
#
# Special-cased workloads (handled out of the per-VM loop):
#   - boot_cold:    timed inside the orchestrator (setup→start→wait_ssh→destroy)
#   - boot_warm:    snapshot→restore→wait_ssh
#   - density:      driven by workloads/density.sh, owns its own VM lifecycle
#   - mgmt_latency: driven by workloads/mgmt_latency.sh
#
# Everything else: shared per-hypervisor VM, workload script invoked over SSH.
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "$0")/lib/common.sh"
# shellcheck disable=SC1091
source "$(dirname "$0")/lib/ssh.sh"

usage() {
    cat >&2 <<EOF
usage: $0 --config <path> --out <dir> [--only-hv NAME ...] [--only-wl NAME ...]
EOF
    exit 2
}

CONFIG=""
OUT=""
ONLY_HV=()
ONLY_WL=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --config)  CONFIG="$2"; shift 2 ;;
        --out)     OUT="$2"; shift 2 ;;
        --only-hv) ONLY_HV+=("$2"); shift 2 ;;
        --only-wl) ONLY_WL+=("$2"); shift 2 ;;
        -h|--help) usage ;;
        *) die "unknown arg: $1" ;;
    esac
done
[[ -n "$CONFIG" && -n "$OUT" ]] || usage
[[ -f "$CONFIG" ]] || die "no such config: $CONFIG"

mkdir -p "$OUT"
cp "$CONFIG" "$OUT/config.snapshot.toml"
capture_env "$OUT"

# --- load config ---
SAMPLES=$(toml_get "$CONFIG" run.samples)
COOLDOWN=$(toml_get "$CONFIG" run.cooldown_sec)
BUDGET=$(toml_get "$CONFIG" run.budget_sec)
IMAGE=$(toml_get "$CONFIG" guest.image)
[[ "$IMAGE" = /* ]] || IMAGE="$BENCH_DIR/$IMAGE"
SSH_USER=$(toml_get "$CONFIG" guest.ssh_user)
SSH_KEY=$(toml_get "$CONFIG" guest.ssh_key)
[[ "$SSH_KEY" = /* ]] || SSH_KEY="$BENCH_DIR/$SSH_KEY"
CPUS=$(toml_get "$CONFIG" guest.cpus)
MEM=$(toml_get "$CONFIG" guest.mem_mib)
NETIF=$(toml_get "$CONFIG" guest.netif)

export BENCH_SSH_USER="$SSH_USER" BENCH_SSH_KEY="$SSH_KEY"
export BENCH_GUEST_IMAGE="$IMAGE" BENCH_GUEST_CPUS="$CPUS" BENCH_GUEST_MEM_MIB="$MEM" BENCH_GUEST_NETIF="$NETIF"
export BENCH_STATE_ROOT="$OUT/.state"

mapfile -t HVS < <(toml_enabled_keys "$CONFIG" hypervisor)
mapfile -t WLS < <(toml_enabled_keys "$CONFIG" workload)

in_filter() {
    local name="$1"; shift
    [[ $# -eq 0 ]] && return 0
    for x in "$@"; do [[ "$x" == "$name" ]] && return 0; done
    return 1
}

# Map a workload key → its workload script + env exports (read from TOML).
workload_env() {
    local wl="$1"
    case "$wl" in
        cpu_single|cpu_multi)
            local t d
            t=$(toml_get "$CONFIG" "workload.${wl}.threads")
            d=$(toml_get "$CONFIG" "workload.${wl}.duration")
            [[ "$t" == "0" ]] && t="$CPUS"
            echo "THREADS=$t DURATION=$d"
            ;;
        mem_bandwidth)
            echo "DURATION=$(toml_get "$CONFIG" workload.mem_bandwidth.duration)" ;;
        disk_rand_read_4k)
            echo "PATTERN=randread BS=4k IODEPTH=$(toml_get "$CONFIG" workload.disk_rand_read_4k.iodepth) SIZE_MIB=$(toml_get "$CONFIG" workload.disk_rand_read_4k.size_mib) NAME=randread_4k" ;;
        disk_rand_write_4k)
            local f; f=$(toml_get "$CONFIG" workload.disk_rand_write_4k.fsync); [[ "$f" == "true" ]] && f=1 || f=0
            echo "PATTERN=randwrite BS=4k IODEPTH=$(toml_get "$CONFIG" workload.disk_rand_write_4k.iodepth) SIZE_MIB=$(toml_get "$CONFIG" workload.disk_rand_write_4k.size_mib) FSYNC=$f NAME=randwrite_4k" ;;
        disk_seq_read_1m)
            echo "PATTERN=read BS=1M IODEPTH=$(toml_get "$CONFIG" workload.disk_seq_read_1m.iodepth) SIZE_MIB=$(toml_get "$CONFIG" workload.disk_seq_read_1m.size_mib) NAME=seqread_1m" ;;
        net_throughput)
            echo "DURATION=$(toml_get "$CONFIG" workload.net_throughput.duration) PARALLEL=$(toml_get "$CONFIG" workload.net_throughput.parallel)" ;;
        net_latency)
            echo "DURATION=$(toml_get "$CONFIG" workload.net_latency.duration)" ;;
        vmexit)
            echo "ITERATIONS=$(toml_get "$CONFIG" workload.vmexit.iterations)" ;;
        kernel_build)
            local j; j=$(toml_get "$CONFIG" workload.kernel_build.make_jobs); [[ "$j" == "0" ]] && j="$CPUS"
            echo "KERNEL_URL=$(toml_get "$CONFIG" workload.kernel_build.kernel_url) MAKE_JOBS=$j" ;;
        density)
            echo "COUNT=$(toml_get "$CONFIG" workload.density.count) SETTLE_S=$(toml_get "$CONFIG" workload.density.settle_s)" ;;
        mgmt_latency)
            echo "CYCLES=$(toml_get "$CONFIG" workload.mgmt_latency.cycles)" ;;
        *) echo "" ;;
    esac
}

workload_script() {
    case "$1" in
        cpu_single|cpu_multi) echo "cpu_sysbench.sh" ;;
        mem_bandwidth)        echo "mem_sysbench.sh" ;;
        disk_rand_read_4k|disk_rand_write_4k|disk_seq_read_1m) echo "disk_fio.sh" ;;
        net_throughput)       echo "net_iperf3.sh" ;;
        net_latency)          echo "net_latency.sh" ;;
        vmexit)               echo "vmexit.sh" ;;
        kernel_build)         echo "kernel_build.sh" ;;
        density)              echo "density.sh" ;;
        mgmt_latency)         echo "mgmt_latency.sh" ;;
        *) echo "" ;;
    esac
}

# Append TSV stdout lines into per-metric raw files under <out>/<hv>/<wl>/raw/.
collect_sample() {
    local hv="$1" wl="$2" sample="$3" raw_dir="$4"
    mkdir -p "$raw_dir"
    while IFS=$'\t' read -r metric value unit; do
        [[ -z "$metric" ]] && continue
        printf '%s\n' "$value"   >> "$raw_dir/${metric}.raw"
        printf '%s\n' "$unit"    >  "$raw_dir/${metric}.unit"
    done
}

# ----- main loops -----
host_prep
trap host_restore EXIT

start_run=$(date +%s)
for hv_name in "${HVS[@]}"; do
    in_filter "$hv_name" "${ONLY_HV[@]:-}" || continue
    log_info "==> hypervisor: $hv_name"
    hv_out="$OUT/$hv_name"; mkdir -p "$hv_out"

    for wl in "${WLS[@]}"; do
        in_filter "$wl" "${ONLY_WL[@]:-}" || continue
        elapsed=$(( $(date +%s) - start_run ))
        if (( elapsed > BUDGET )); then
            log_warn "budget exceeded ($elapsed > $BUDGET s); stopping"
            break 2
        fi

        log_info "  -- workload: $wl"
        wl_dir="$hv_out/$wl"; raw_dir="$wl_dir/raw"
        mkdir -p "$raw_dir"

        env_kvs=$(workload_env "$wl")
        script=$(workload_script "$wl")

        # ---- workload class A: orchestrator-owned (special) ----
        case "$wl" in
          boot_cold)
            for i in $(seq 0 "$SAMPLES"); do  # 0 = warm-up, discarded
                vm=$(hv "$hv_name" setup "$IMAGE" "$CPUS" "$MEM" "" "$NETIF")
                pid=$(hv "$hv_name" start "$vm")
                ssh_port=$(. "$BENCH_STATE_ROOT/$hv_name/$vm/meta"; echo "$ssh_port")
                if elapsed=$(hv "$hv_name" wait_ssh "$vm" "$ssh_port" 120); then
                    if (( i > 0 )); then
                        printf '%s\n' "$elapsed" >> "$raw_dir/boot_cold_seconds.raw"
                        echo s > "$raw_dir/boot_cold_seconds.unit"
                    fi
                else
                    log_warn "    boot_cold sample $i: SSH timeout"
                fi
                hv "$hv_name" stop "$vm"    >/dev/null 2>&1 || true
                hv "$hv_name" destroy "$vm" >/dev/null 2>&1 || true
                sleep "$COOLDOWN"
            done
            ;;

          boot_warm)
            vm=$(hv "$hv_name" setup "$IMAGE" "$CPUS" "$MEM" "" "$NETIF")
            pid=$(hv "$hv_name" start "$vm")
            ssh_port=$(. "$BENCH_STATE_ROOT/$hv_name/$vm/meta"; echo "$ssh_port")
            hv "$hv_name" wait_ssh "$vm" "$ssh_port" 120 >/dev/null
            if ! hv "$hv_name" snapshot "$vm" warm; then
                log_warn "    snapshot unsupported on $hv_name; skipping boot_warm"
                hv "$hv_name" stop "$vm"; hv "$hv_name" destroy "$vm"
                continue
            fi
            hv "$hv_name" stop "$vm"
            for i in $(seq 0 "$SAMPLES"); do
                t0=$(date +%s.%N)
                hv "$hv_name" restore "$vm" warm >/dev/null
                if elapsed=$(hv "$hv_name" wait_ssh "$vm" "$ssh_port" 60); then
                    if (( i > 0 )); then
                        printf '%s\n' "$elapsed" >> "$raw_dir/boot_warm_seconds.raw"
                        echo s > "$raw_dir/boot_warm_seconds.unit"
                    fi
                fi
                hv "$hv_name" stop "$vm" >/dev/null 2>&1 || true
                sleep "$COOLDOWN"
            done
            hv "$hv_name" destroy "$vm"
            ;;

          density|mgmt_latency)
            # Driven entirely by the workload script.
            log_info "    invoking $script (self-driven)"
            HV="$hv_name" env $env_kvs bash "$WORKLOADS_DIR/$script" 2>"$wl_dir/stderr.log" \
                | tee "$wl_dir/stdout.log" \
                | collect_sample "$hv_name" "$wl" 1 "$raw_dir"
            ;;

          # ---- workload class B: per-sample over a shared VM ----
          *)
            [[ -z "$script" ]] && { log_warn "no script for workload $wl; skipping"; continue; }
            vm=$(hv "$hv_name" setup "$IMAGE" "$CPUS" "$MEM" "" "$NETIF")
            pid=$(hv "$hv_name" start "$vm")
            ssh_port=$(. "$BENCH_STATE_ROOT/$hv_name/$vm/meta"; echo "$ssh_port")
            if ! hv "$hv_name" wait_ssh "$vm" "$ssh_port" 180 >/dev/null; then
                log_error "    VM never became SSH-ready; skipping workload"
                hv "$hv_name" stop "$vm"; hv "$hv_name" destroy "$vm"
                continue
            fi
            export BENCH_SSH_HOST=127.0.0.1 BENCH_SSH_PORT="$ssh_port"
            # Warm-up.
            env $env_kvs bash "$WORKLOADS_DIR/$script" >/dev/null 2>&1 || true
            for i in $(seq 1 "$SAMPLES"); do
                log_info "    sample $i/$SAMPLES"
                env $env_kvs bash "$WORKLOADS_DIR/$script" \
                    2>>"$wl_dir/stderr.log" \
                    | tee -a "$wl_dir/stdout.log" \
                    | collect_sample "$hv_name" "$wl" "$i" "$raw_dir"
                sleep "$COOLDOWN"
            done
            hv "$hv_name" stop "$vm"
            hv "$hv_name" destroy "$vm"
            ;;
        esac

        # Summarize per metric.
        for raw in "$raw_dir"/*.raw; do
            [[ -f "$raw" ]] || continue
            metric=$(basename "$raw" .raw)
            summarize_csv "$raw" "$wl_dir/${metric}.summary.csv"
        done
    done
done

log_info "run complete: $OUT"
