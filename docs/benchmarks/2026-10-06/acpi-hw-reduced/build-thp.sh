#!/usr/bin/env bash
# Build one daemon variant ($1 = baseline|candidate) from HM-trial's current tree.
# The caller stashes/unstashes on the Windows side: git in WSL cannot read this worktree.
set -e
export RUSTUP_HOME=/var/tmp/rustup CARGO_HOME=/var/tmp/cargo-home PATH=/var/tmp/cargo-home/bin:/usr/local/bin:/usr/bin:/bin
export CARGO_TARGET_DIR=/var/tmp/hm-trial-target PROTOC=$(command -v protoc || echo /var/tmp/protoc/bin/protoc)
cd /path/to/HyperMachine
touch crates/hv2-core/src/backends/kvm.rs
cargo build --release -q -p hv2-sandboxd 2>&1 | tail -5
cp $CARGO_TARGET_DIR/release/hv2-sandboxd /var/tmp/hm-thp-$1
echo "$1: MADV_HUGEPAGE strings $(grep -c 'MADV_HUGEPAGE' /var/tmp/hm-thp-$1 || true) sha $(sha256sum /var/tmp/hm-thp-$1 | cut -c1-16)"
