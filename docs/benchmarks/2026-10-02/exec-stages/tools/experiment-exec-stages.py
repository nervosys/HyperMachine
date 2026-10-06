#!/usr/bin/env python3
"""Instrument command timing in a hash-verified isolated buffer-mode source copy."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


def replace(text, anchor, replacement):
    if text.count(anchor) != 1:
        raise ValueError('execution-stage anchor differs: ' + anchor)
    return text.replace(anchor, replacement)


def generate(root, accepted, modes, patch):
    root = root.resolve(strict=True)
    if root == Path(__file__).resolve().parent.parent or (root / '.git').exists():
        raise ValueError('requires isolated source copy')
    catalog = json.loads(accepted.read_text())['accepted_source_sha256']
    mode_catalog = json.loads(modes.read_text())['mode_source_sha256']
    expected = {**catalog, **mode_catalog}
    for name, sha in expected.items():
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('mode source binding differs: ' + name)
    agent = 'crates/hv2-agent/src/agent_vm.rs'
    daemon = 'crates/hv2-sandboxd/src/main.rs'
    original = {n: (root / n).read_bytes().decode('utf-8') for n in (agent, daemon)}
    changed = dict(original)
    anchor = '''        let program = program.to_string();
        let args = args.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut agent = GuestAgent::over_vsock(device, timeout)?;
            agent.exec(&program, &args, timeout)
        })'''
    changed[agent] = replace(changed[agent], anchor, '''        let program = program.to_string();
        let args = args.to_vec();
        let vm_name = self.vm.config().name.clone();
        let queued_at = std::time::Instant::now();
        tokio::task::spawn_blocking(move || {
            let started_at = std::time::Instant::now();
            let mut agent = match GuestAgent::over_vsock(device, timeout) {
                Ok(agent) => agent,
                Err(error) => {
                    tracing::debug!(vm = %vm_name,
                        blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                        connect_ms = started_at.elapsed().as_secs_f64() * 1000.0,
                        command_ms = 0.0, succeeded = false, phase = "connect",
                        "guest command execution stages");
                    return Err(error);
                }
            };
            let connected_at = std::time::Instant::now();
            let result = agent.exec(&program, &args, timeout);
            tracing::debug!(vm = %vm_name,
                blocking_queue_ms = (started_at - queued_at).as_secs_f64() * 1000.0,
                connect_ms = (connected_at - started_at).as_secs_f64() * 1000.0,
                command_ms = connected_at.elapsed().as_secs_f64() * 1000.0,
                succeeded = result.is_ok(), phase = "command",
                "guest command execution stages");
            result
        })''')
    anchor = '''    Json(req): Json<ExecRequest>,
) -> Response {
    let (vm, _active) = {'''
    changed[daemon] = replace(changed[daemon], anchor, '''    Json(req): Json<ExecRequest>,
) -> Response {
    let handler_started = std::time::Instant::now();
    let (vm, _active) = {''')
    anchor = '''    let result = vm
        .exec_in_guest(
            "/bin/sh",'''
    changed[daemon] = replace(changed[daemon], anchor, '''    let execution_started = std::time::Instant::now();
    let result = vm
        .exec_in_guest(
            "/bin/sh",''')
    anchor = '''        .await;

    match result {
        Ok(exec) => Json(ExecResponse {'''
    changed[daemon] = replace(changed[daemon], anchor, '''        .await;
    let execution_elapsed = execution_started.elapsed();
    tracing::debug!(vm = %sandbox_id,
        handler_ms = handler_started.elapsed().as_secs_f64() * 1000.0,
        guest_exec_ms = execution_elapsed.as_secs_f64() * 1000.0,
        succeeded = result.is_ok(), "sandbox command handler stages");

    match result {
        Ok(exec) => Json(ExecResponse {''')
    if patch.exists():
        raise ValueError('preserve existing instrumentation patch')
    patch.write_bytes(''.join(''.join(difflib.unified_diff(original[n].splitlines(True), changed[n].splitlines(True), fromfile='mode/' + n, tofile='diagnostic/' + n)) for n in changed).encode('utf-8'))
    for name, text in changed.items():
        (root / name).write_bytes(text.encode('utf-8'))
    for name, sha in expected.items():
        if name not in changed and hashlib.sha256((root / name).read_bytes()).hexdigest() != sha:
            raise ValueError('unrelated source changed: ' + name)
    return {'diagnostic_only': True, 'performance_win_established': False, 'production_runtime_changed': False,
            'source_files_verified': len(expected), 'mode_source_sha256': mode_catalog,
            'exec_source_sha256': {n: hashlib.sha256((root / n).read_bytes()).hexdigest() for n in changed},
            'trace_payload': 'VM identity, durations, success and phase only; no command, args, output, entropy or credentials'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--accepted-context', type=Path, required=True)
    parser.add_argument('--mode-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.accepted_context, args.mode_context, args.patch), indent=2))
