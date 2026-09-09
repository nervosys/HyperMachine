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
# Not a workspace member, so --all does not reach it.
if (cd crates/hv1-multiboot && cargo fmt -- --check >/dev/null 2>&1); then
    echo "  hv1-multiboot clean"
else
    bad "cargo fmt in crates/hv1-multiboot"
fi

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
# CI runs none of these, and they are where every measured claim comes from.
run() {
    local name="$1"
    shift
    local out code
    out=$(timeout 600 cargo run --release "$@" 2>&1)
    code=$?
    local failed
    failed=$(echo "$out" | grep -c FAILED)
    printf '  %-16s exit=%d FAILED=%d\n' "$name" "$code" "$failed"
    { [ "$code" -eq 0 ] && [ "$failed" -eq 0 ]; } || bad "example $name"
}

run unikernel_boot -p hv2-core --example unikernel_boot
run rust_unikernel -p hv2-core --example rust_unikernel
run multiboot_probe -p hv2-core --example multiboot_probe
run vsock_echo -p hv2-core --example vsock_echo
run halt_stop_probe -p hv2-core --example halt_stop_probe
run hv1_under_hv2 -p hv2-core --example hv1_under_hv2
run tool_calls -p hv2-swarm --example tool_calls
run agent_messages -p hv2-swarm --example agent_messages
run vsock_swarm -p hv2-swarm --example vsock_swarm
run in_flight -p hv2-swarm --example in_flight

if [ -n "$MODEL" ] && [ -f "$MODEL" ]; then
    run bandwidth -p hv2-infer --example bandwidth -- "$MODEL"
    run batched -p hv2-infer --example batched -- "$MODEL"
    run generate -p hv2-infer --example generate -- "$MODEL"
    run queueing -p hv2-infer --example queueing -- "$MODEL"
    run inference -p hv2-swarm --example inference -- "$MODEL"
    run scheduled -p hv2-swarm --example scheduled -- "$MODEL"
else
    echo "  (six examples skipped: no model given, so nothing here checked"
    echo "   inference, the queue, or a swarm with weights in it)"
fi

# A killed or truncated run prints no marker, and a run that printed no marker
# did not finish. Three separate results were nearly believed this way.
if [ "$fail" -eq 0 ]; then
    printf '\nSWEEP OK\n'
else
    printf '\nSWEEP FAILED\n'
fi
exit "$fail"
