#!/usr/bin/env bash
#
# Everything that has to be true before a change is believed.
#
# This existed only in a session transcript until now, which meant the procedure
# that checks every claim in docs/handoff.html was itself unversioned, and each
# run of it had to be reconstructed from memory. Several of the findings on that
# page are things this script does that CI does not.
#
# It is a Linux script and it wants WSL on this host, because `/dev/kvm` is
# there. The Windows half is checked separately; see `tools/sweep.ps1`.
#
#   tools/sweep.sh /path/to/model.gguf
#
# The model argument is optional. Without it the six examples that need weights
# are skipped and the run says so rather than quietly passing.

set -uo pipefail

MODEL="${1:-}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO" || exit 1

# A target directory of its own. Sharing `target/` with the Windows build makes
# each invalidate the other's artifacts, which turns a two-minute check into a
# twenty-minute one.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/var/tmp/hm-target}"
# hv2-api's build script needs this and WSL has no system protoc.
export PROTOC="${PROTOC:-/var/tmp/protoc/bin/protoc}"

# How many test-result lines a healthy run produces, derived rather than
# observed:
#
#   18 lib unittests + 48 integration + 4 bin unittests + 18 doc-suites = 88
#
# Four binaries and not five because hv1-core's `hv1-kernel` carries
# `required-features = ["bootloader_api"]`, so a default build never makes it.
# `cargo metadata` is the source for all of these. If this number is wrong the
# run says so: a count that drifts silently is how 3,331 tests once got
# reported as a pass with whole crates missing from the run.
EXPECT_RESULT_LINES="${EXPECT_RESULT_LINES:-88}"

# Below this many test binaries something is structurally wrong -- a crate
# failed to build, or the invocation was not what it looked like -- and no
# summary should be printed at all.
MIN_BINARIES=20

fail=0
step() { printf '\n=== %s\n' "$1"; }
bad() { echo "  FAILED: $1"; fail=1; }

step "fmt"
if cargo fmt --all -- --check >/dev/null 2>&1; then
    echo "  workspace clean"
else
    bad "cargo fmt --all -- --check"
fi
# The three crates in the root manifest's `exclude` list. `--all` means "every
# workspace member", so none of them is reached by the command above, and until
# this loop existed only hv1-multiboot was checked by anything: hv2-unikernel
# was silently unformatted, and hv1-boot's manifest could not be parsed at all.
#
# rustfmt is the one check that works on all three. They target bare metal, so
# there is no host build to run clippy against, and CI builds them by name with
# their own targets and toolchains.
for excluded in hv1-multiboot hv2-unikernel hv1-boot; do
    if cargo fmt --manifest-path "crates/$excluded/Cargo.toml" -- --check >/dev/null 2>&1; then
        echo "  $excluded clean"
    else
        bad "cargo fmt in crates/$excluded (a manifest that will not parse fails here too)"
    fi
done

step "clippy"
# --all-targets, which is what CI does not do: it is how tests, examples and
# benches get linted rather than only the libraries.
n=$(cargo clippy --release --workspace --all-targets -- -D warnings 2>&1 |
    grep -cE '^(warning|error)')
echo "  workspace: $n"
[ "$n" -eq 0 ] || bad "clippy reported $n"
n=$(cd crates/hv1-multiboot && cargo clippy --release -- -D warnings 2>&1 |
    grep -cE '^(warning|error)')
echo "  hv1-multiboot: $n"
[ "$n" -eq 0 ] || bad "clippy reported $n in hv1-multiboot"

step "doc"
n=$(RUSTDOCFLAGS="-D warnings" CARGO_TARGET_DIR=/var/tmp/hm-doc \
    cargo doc --workspace --no-deps 2>&1 | grep -cE '^error')
echo "  errors: $n"
[ "$n" -eq 0 ] || bad "rustdoc reported $n errors"

step "tests"
log="$(mktemp)"
cargo test --release --workspace >"$log" 2>&1
code=$?
lines=$(grep -cE '^test result' "$log")
if [ "$lines" -lt "$MIN_BINARIES" ]; then
    echo "  BROKEN: only $lines result lines; refusing to summarise"
    grep -E '^error' "$log" | head -5
    fail=1
else
    p=$(grep -E '^test result' "$log" | grep -oE '[0-9]+ passed' |
        awk '{s+=$1} END {print s+0}')
    f=$(grep -E '^test result' "$log" | grep -oE '[0-9]+ failed' |
        awk '{s+=$1} END {print s+0}')
    i=$(grep -E '^test result' "$log" | grep -oE '[0-9]+ ignored' |
        awk '{s+=$1} END {print s+0}')
    echo "  $p passed, $f failed, $i ignored, across $lines result lines"
    [ "$code" -eq 0 ] || bad "cargo test exited $code"
    [ "$f" -eq 0 ] || bad "$f tests failed"
    if [ "$lines" -ne "$EXPECT_RESULT_LINES" ]; then
        bad "expected $EXPECT_RESULT_LINES result lines, saw $lines -- a target
       appeared or vanished, which is a fact about the build and not about the
       tests. Check \`cargo metadata\` before changing the expectation."
    fi
fi
rm -f "$log"

step "examples"
# Discovered, not listed.
#
# This was a hardcoded list of sixteen, and the flaw showed up the way these
# things do: somebody added a seventeenth example and the sweep went on
# reporting success without it. Worse, the sixteen were never all of them --
# `cargo metadata` reports 51 example targets, so a list maintained by hand was
# covering a third of them and saying nothing about the rest.
#
# So the set is read from cargo, and anything new is run by default. That is the
# safe direction: a new example should have to opt *out* of being checked, not
# opt in.
EXPECT_EXAMPLES="${EXPECT_EXAMPLES:-51}"

# The ones that take a model path. Everything else is run with no arguments.
MODEL_EXAMPLES=" bandwidth batched generate queueing inference scheduled throughput "

# Of those, the ones a *synthetic* model is enough for.
#
# Without a real model all seven used to be skipped, and the run said so --
# honest, but it left a seventh of the examples unchecked on every CI machine
# that has no 1.2 GB GGUF on it. `tools/mk-test-gguf.py` writes a 359 KiB llama
# that loads and runs and says nothing sensible, which is exactly enough for
# the three below: they assert on the machinery, not on the answer. Queue
# fairness, streaming bandwidth and forward-pass rate do not care whether the
# weights were ever trained.
#
# Measured, not assumed -- each of these was run against the fixture and exits
# 0, and the other four were run against it too and fail correctly:
#
#   generate   FAILED -- the model produced tokens and not the answer
#   batched    FAILED -- the batched answer is not "8"
#   inference  asserts "Paris" and "Blue" at each guest's own console
#   scheduled  asserts an agent recalled a number from its own first turn
#
# Those four want weights that were actually trained, and no fixture fixes
# that. They stay skipped unless a real model is passed.
#
# The numbers `throughput` and `bandwidth` print with the fixture are
# meaningless -- a 359 KiB model measures the loop overhead and nothing else.
# What they check here is that the code path runs, which is what a sweep is
# for. Benchmarks are quoted from a real model or not at all.
SYNTHETIC_OK=" bandwidth queueing throughput "

# Known not to run unattended here, each with its reason. This is the one place
# a failure can hide, so every entry names why, and the reasons were measured
# rather than assumed -- an earlier version of this list called eight of these
# "long-running demos" on the strength of a 90-second timeout that was being
# spent on rustc rather than on the example. Built first, they all finish
# inside a second, five of them cleanly. Those five run now.
#
#   pic_timer_interrupts  Windows-gated; tools/sweep.ps1 builds it
#   linux_boot_probe      wants a bzImage argument
#   guest_exec_probe      wants a bzImage and an initramfs
#
# `advanced` and `basic` used to be here, blamed on "pause a VM that has no
# guest". That diagnosis was wrong, and the way it was wrong is worth keeping.
# It is not about the guest: `VM::pause` pauses each vCPU, `VCpu::pause`
# requires `VCpuState::Running`, and **nothing in this repository ever writes
# that state** -- it appears exactly once, in the comparison that rejects. So
# pause() could never succeed for any VM, and none ever has been paused. Both
# examples now print the refusal instead of propagating it, and pause/resume
# say plainly that suspend-and-continue is unimplemented. They run.
#
# `exit_handling`, `interrupt_demo` and `vm_with_interrupts` used to be here,
# as one defect rather than three: `create_vm` calls `kvm_create_irqchip`, so
# the PIC is in the kernel, and `inject_interrupt` issues KVM_INTERRUPT, which
# KVM accepts only when the irqchip is in userspace. Fixing them meant deciding
# which of the two irqchips is real.
#
# It is the in-kernel one, and that was not close: `create_vm` builds it on
# every VM, `vm.rs` delivers every device interrupt through `set_irq_line`, and
# every example that runs a guest depends on it. So the three examples were
# wrong, not the hypervisor, and they now raise lines instead of vectors. They
# run.
SKIP=" pic_timer_interrupts linux_boot_probe guest_exec_probe "

examples=$(cargo metadata --format-version 1 --no-deps 2>/dev/null |
    python3 -c "
import json,sys
md = json.load(sys.stdin)
for p in md['packages']:
    for t in p['targets']:
        if t['kind'] == ['example']:
            print(p['name'], t['name'])
" | sort -k2)

found=$(printf '%s\n' "$examples" | grep -c .)
echo "  $found example targets"
if [ "$found" -ne "$EXPECT_EXAMPLES" ]; then
    bad "expected $EXPECT_EXAMPLES example targets, found $found -- an example
       was added or removed. Run it, decide which list it belongs in, and move
       this number."
fi

run() {
    local name="$1"
    shift
    local out code failed
    out=$(timeout 600 cargo run --release "$@" 2>&1)
    code=$?
    failed=$(echo "$out" | grep -c FAILED)
    printf '  %-24s exit=%d FAILED=%d\n' "$name" "$code" "$failed"
    { [ "$code" -eq 0 ] && [ "$failed" -eq 0 ]; } || bad "example $name"
}

# A stand-in model, when no real one was given. Written to the target
# directory rather than the repository: it is a build artifact, it is
# deterministic, and a 359 KiB binary in git would be a 359 KiB binary in every
# clone forever.
FIXTURE=""
if [ -z "$MODEL" ] || [ ! -f "$MODEL" ]; then
    candidate="$CARGO_TARGET_DIR/tiny-llama.gguf"
    if [ -f "$candidate" ]; then
        FIXTURE="$candidate"
    elif command -v python3 >/dev/null 2>&1 &&
        python3 tools/mk-test-gguf.py "$candidate" >/dev/null 2>&1; then
        FIXTURE="$candidate"
    else
        echo "  (no python3, so no synthetic model either)"
    fi
fi

ran=0
skipped=0
while read -r pkg name; do
    [ -z "$name" ] && continue
    case "$SKIP" in *" $name "*) skipped=$((skipped + 1)); continue ;; esac
    case "$MODEL_EXAMPLES" in
        *" $name "*)
            if [ -n "$MODEL" ] && [ -f "$MODEL" ]; then
                run "$name" -p "$pkg" --example "$name" -- "$MODEL"
                ran=$((ran + 1))
            elif [ -n "$FIXTURE" ]; then
                case "$SYNTHETIC_OK" in
                    *" $name "*)
                        run "$name" -p "$pkg" --example "$name" -- "$FIXTURE"
                        ran=$((ran + 1))
                        ;;
                    *) skipped=$((skipped + 1)) ;;
                esac
            else
                skipped=$((skipped + 1))
            fi
            ;;
        *)
            run "$name" -p "$pkg" --example "$name"
            ran=$((ran + 1))
            ;;
    esac
done <<EOF
$examples
EOF

echo "  ran $ran, skipped $skipped of $found"
if [ -n "$MODEL" ] && [ -f "$MODEL" ]; then
    :
elif [ -n "$FIXTURE" ]; then
    echo "  (a synthetic model stood in, so the queue and the forward pass were"
    echo "   exercised but nothing checked that the weights can answer; the four"
    echo "   examples that need trained weights were skipped, and any rate"
    echo "   printed above is the fixture's, not this machine's)"
else
    echo "  (no model given and no fixture could be built, so nothing here"
    echo "   checked inference, the queue, or a swarm with weights in it)"
fi

# A killed or truncated run prints no marker, and a run that printed no marker
# did not finish. Three separate results were nearly believed this way.
if [ "$fail" -eq 0 ]; then
    printf '\nSWEEP OK\n'
else
    printf '\nSWEEP FAILED\n'
fi
exit "$fail"
