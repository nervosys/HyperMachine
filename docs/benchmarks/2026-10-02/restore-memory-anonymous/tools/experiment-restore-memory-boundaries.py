#!/usr/bin/env python3
"""Generate opt-in per-VM memory boundary probes in isolated accepted sources."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

PROBE = '''    /// Isolated diagnostic only: no contents or host addresses are logged.
    pub fn diagnose_restore_memory(&self, phase: &str) -> Result<()> {
        if std::env::var("HM_RESTORE_MEMORY_DIAGNOSTICS").as_deref() != Ok("1") {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            use std::io::Read;
            let started = std::time::Instant::now();
            let regions = self.memory.regions();
            if regions.len() != 1 {
                return Err(Error::Memory("memory diagnostic requires one guest RAM region".into()));
            }
            let region = &regions[0];
            let end = region.host_addr.checked_add(region.size)
                .ok_or_else(|| Error::Memory("diagnostic address overflow".into()))?;
            let mut text = String::new();
            std::fs::File::open("/proc/self/smaps")
                .and_then(|file| file.take(16 * 1024 * 1024 + 1).read_to_string(&mut text))
                .map_err(|e| Error::Memory(format!("diagnostic smaps read: {e}")))?;
            if text.len() > 16 * 1024 * 1024 {
                return Err(Error::Memory("diagnostic smaps exceeds bound".into()));
            }
            let mut selected = false;
            let mut matches = 0;
            let mut fields = std::collections::BTreeMap::new();
            for line in text.lines() {
                let first = line.split_whitespace().next().unwrap_or("");
                if let Some((lo, hi)) = first.split_once('-') {
                    if let (Ok(lo), Ok(hi)) = (u64::from_str_radix(lo, 16), u64::from_str_radix(hi, 16)) {
                        selected = lo == region.host_addr && hi == end;
                        if selected { matches += 1; }
                        continue;
                    }
                }
                if selected {
                    if let Some((key, value)) = line.split_once(':') {
                        if ["Size", "Rss", "Pss", "Private_Dirty", "Shared_Clean", "Shared_Dirty", "Anonymous", "Private_Clean"].contains(&key) {
                            let mut words = value.split_whitespace();
                            let amount = words.next().and_then(|v| v.parse::<u64>().ok());
                            if words.next() != Some("kB") || words.next().is_some() || amount.is_none()
                                || fields.insert(key, amount.unwrap_or(0)).is_some() {
                                return Err(Error::Memory("malformed diagnostic smaps field".into()));
                            }
                        }
                    }
                }
            }
            if matches != 1 || fields.len() != 8 || fields.get("Size") != Some(&(region.size / 1024)) {
                return Err(Error::Memory("diagnostic guest mapping not found exactly once".into()));
            }
            tracing::warn!(vm = %self.config.name, phase,
                size_kib = fields["Size"], rss_kib = fields["Rss"], pss_kib = fields["Pss"],
                private_dirty_kib = fields["Private_Dirty"], shared_clean_kib = fields["Shared_Clean"],
                shared_dirty_kib = fields["Shared_Dirty"], anonymous_kib = fields["Anonymous"],
                private_clean_kib = fields["Private_Clean"], read_us = started.elapsed().as_micros() as u64,
                "restore memory boundary diagnostic");
        }
        Ok(())
    }

'''

def replace(text, anchor, replacement):
    if text.count(anchor) != 1:
        raise ValueError('boundary probe anchor differs: ' + anchor)
    return text.replace(anchor, replacement)

def generate(root, context, patch):
    root = root.resolve(strict=True)
    if root == Path(__file__).resolve().parent.parent or (root / '.git').exists():
        raise ValueError('requires isolated source copy')
    catalog = json.loads(context.read_text())['accepted_source_sha256']
    for name, sha in catalog.items():
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('accepted source differs: ' + name)
    vm = 'crates/hv2-core/src/vm.rs'
    agent = 'crates/hv2-agent/src/agent_vm.rs'
    original = {n: (root / n).read_bytes().decode('utf-8') for n in (vm, agent)}
    changed = dict(original)
    anchor = '    /// Get guest memory\n    pub fn memory(&self) -> Arc<GuestMemory> {'
    changed[vm] = replace(changed[vm], anchor, PROBE + anchor)
    anchor = '        let restored = t0.elapsed();\n        self.start_in_background().await?;'
    changed[vm] = replace(changed[vm], anchor, '        let restored = t0.elapsed();\n        self.diagnose_restore_memory("before_run")?;\n        self.start_in_background().await?;')
    anchor = '            let mut agent = GuestAgent::over_vsock(device, timeout)?;\n            agent.exec(&program, &args, timeout)'
    changed[agent] = replace(changed[agent], '        let args = args.to_vec();\n        tokio::task::spawn_blocking(move || {\n' + anchor,
        '        let args = args.to_vec();\n        let diagnostic_vm = Arc::clone(&self.vm);\n        tokio::task::spawn_blocking(move || {\n            let mut agent = GuestAgent::over_vsock(device, timeout)?;\n            let result = agent.exec(&program, &args, timeout);\n            if result.is_ok() { diagnostic_vm.diagnose_restore_memory("after_exec")?; }\n            result')
    anchor = '            let result = agent.restored_now(entropy, timeout);'
    changed[agent] = replace(changed[agent], '        let queued_at = std::time::Instant::now();\n        let reseeded =', '        let queued_at = std::time::Instant::now();\n        let diagnostic_vm = Arc::clone(&self.vm);\n        let reseeded =')
    changed[agent] = replace(changed[agent], anchor, anchor + '\n            if result.is_ok() { diagnostic_vm.diagnose_restore_memory("after_notice")?; }')
    if patch.exists():
        raise ValueError('preserve previous patch')
    patch.write_bytes(''.join(''.join(difflib.unified_diff(original[n].splitlines(True), changed[n].splitlines(True), fromfile='accepted/' + n, tofile='diagnostic/' + n)) for n in changed).encode('utf-8'))
    for name, text in changed.items():
        (root / name).write_bytes(text.encode('utf-8'))
    return {'diagnostic_only': True, 'production_runtime_changed': False, 'source_files_verified': len(catalog),
            'source_sha256': {n: hashlib.sha256((root / n).read_bytes()).hexdigest() for n in changed},
            'activation_environment': 'HM_RESTORE_MEMORY_DIAGNOSTICS=1',
            'counter_profile': 'anonymous_private_clean',
            'logged_payload': 'VM identity, phase, residency counters and read duration only'}

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--accepted-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.accepted_context, args.patch), indent=2))
