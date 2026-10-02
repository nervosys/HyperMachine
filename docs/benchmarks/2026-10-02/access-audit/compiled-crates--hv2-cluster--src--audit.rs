//! Durable, credential-free protected API admission and completion records.
use std::io;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use hv2_core::security::audit_chain::AuditChain;
use serde_json::Value;

#[derive(Debug)]
pub struct AccessAudit {
    chain: AuditChain,
    failed: AtomicBool,
    write_lock: parking_lot::Mutex<()>,
}

impl AccessAudit {
    #[cfg(test)]
    pub(crate) fn with_test_chain(chain: AuditChain) -> Arc<Self> {
        Arc::new(Self {
            chain,
            failed: AtomicBool::new(false),
            write_lock: parking_lot::Mutex::new(()),
        })
    }
    pub fn open(path: &Path, key: [u8; 32]) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            chain: AuditChain::open_exclusive_file(path, key)?,
            failed: AtomicBool::new(false),
            write_lock: parking_lot::Mutex::new(()),
        }))
    }

    /// Optional operator configuration. A partial configuration refuses startup.
    /// The key is read from a bounded file; credentials never appear in errors.
    pub fn from_env() -> io::Result<Option<Arc<Self>>> {
        use std::io::Read;
        use zeroize::Zeroize;
        let (path, key_file) = match (
            std::env::var_os("HV2_ACCESS_AUDIT"),
            std::env::var_os("HV2_ACCESS_AUDIT_KEY_FILE"),
        ) {
            (None, None) => return Ok(None),
            (Some(path), Some(key)) => (path, key),
            _ => {
                return Err(io::Error::other(
                    "HV2_ACCESS_AUDIT and HV2_ACCESS_AUDIT_KEY_FILE must be set together",
                ))
            }
        };
        let mut bytes = Vec::new();
        let read =
            std::fs::File::open(key_file).and_then(|file| file.take(129).read_to_end(&mut bytes));
        if read.is_err() {
            bytes.zeroize();
            return Err(io::Error::other("could not read access audit key file"));
        }
        let oversized = bytes.len() > 128;
        let key = std::str::from_utf8(&bytes)
            .ok()
            .map(str::trim)
            .filter(|text| text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(|text| {
                let mut key = [0u8; 32];
                for (index, byte) in key.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                        .expect("checked hex");
                }
                key
            });
        bytes.zeroize();
        let key = key
            .filter(|_| !oversized)
            .ok_or_else(|| io::Error::other("access audit key must contain 64 hex characters"))?;
        Self::open(Path::new(&path), key)
            .map(Some)
            .map_err(|_| io::Error::other("could not exclusively open and verify access audit log"))
    }

    pub async fn append(self: &Arc<Self>, event: Value) -> io::Result<()> {
        let audit = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = audit.write_lock.lock();
            if audit.failed.load(Ordering::Acquire) {
                return Err(io::Error::other("access audit unavailable"));
            }
            if audit.chain.append("control-plane-access", event).is_err() {
                audit.failed.store(true, Ordering::Release);
                return Err(io::Error::other("access audit unavailable"));
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            self.failed.store(true, Ordering::Release);
            io::Error::other("access audit unavailable")
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_core::security::audit_chain::AuditSink;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;

    struct FailingSink(Arc<AtomicUsize>);
    impl AuditSink for FailingSink {
        fn write_line(&mut self, _: &str) -> io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::other(
                "injected failure after an uncertain partial write",
            ))
        }
    }

    #[tokio::test]
    async fn failed_write_permanently_stops_all_queued_and_future_appends() {
        let calls = Arc::new(AtomicUsize::new(0));
        let audit = Arc::new(AccessAudit {
            chain: AuditChain::new([42; 32], Box::new(FailingSink(calls.clone()))),
            failed: AtomicBool::new(false),
            write_lock: parking_lot::Mutex::new(()),
        });
        let mut tasks = Vec::new();
        for n in 0..20 {
            let audit = audit.clone();
            tasks.push(tokio::spawn(
                async move { audit.append(json!({"n":n})).await },
            ));
        }
        for task in tasks {
            assert!(task.await.unwrap().is_err());
        }
        assert!(audit.append(json!({"later":true})).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
