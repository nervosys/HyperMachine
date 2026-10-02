#!/usr/bin/env python3
"""Generate a one-time startup trim candidate only in an isolated source copy."""
import argparse
import difflib
import hashlib
from pathlib import Path

BASE_SHA256='5b8d92902845f748bcfd497cdaa349a7509b4ed27fd6dd867935c8f27e59fb52'
ANCHOR='    let template_line = if templates.is_empty() {'
INSERT='''    // Isolated experiment: reclaim free GNU allocator pages once after
    // initial template preparation, before listening or joining a cluster.
    // No periodic worker or per-request reclaim is installed.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if !templates.is_empty() {
        let started = std::time::Instant::now();
        // SAFETY: malloc_trim takes only a padding value and acts on free
        // allocator chunks. It is thread-safe and runs in normal execution,
        // never a signal handler. Live guest allocations remain allocated.
        let released = unsafe { libc::malloc_trim(0) };
        eprintln!("HV2_STARTUP_RECLAIM_EXPERIMENT released={released} duration_ns={}", started.elapsed().as_nanos());
    }

'''


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('main',type=Path);parser.add_argument('--patch',required=True,type=Path);args=parser.parse_args()
    before=args.main.read_bytes()
    if hashlib.sha256(before).hexdigest()!=BASE_SHA256:raise ValueError('requires exact accepted main in isolated copy')
    text=before.decode();
    if text.count(ANCHOR)!=1:raise ValueError('startup anchor differs')
    changed=text.replace(ANCHOR,INSERT+ANCHOR)
    args.patch.write_text(''.join(difflib.unified_diff(text.splitlines(True),changed.splitlines(True),fromfile='accepted-main.rs',tofile='candidate-main.rs')))
    args.main.write_bytes(changed.encode())
    print(hashlib.sha256(args.main.read_bytes()).hexdigest())
