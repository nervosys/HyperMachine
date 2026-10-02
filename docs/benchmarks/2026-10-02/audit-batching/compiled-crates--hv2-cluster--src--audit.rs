//! Durable, credential-free protected API admission and completion records.
use std::io;
use std::path::Path;
use std::sync::Arc;

use hv2_core::security::audit_chain::AuditChain;
use serde_json::Value;

#[derive(Debug)]
pub struct AccessAudit {
    sender: Option<tokio::sync::mpsc::Sender<PendingRecord>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

struct PendingRecord {
    event: Value,
    done: tokio::sync::oneshot::Sender<bool>,
}

impl Drop for AccessAudit {
    fn drop(&mut self) {
        // Close the queue before joining, so the worker drains and releases
        // its file lock before a replacement writer can start.
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AccessAudit {
    fn with_chain(chain: AuditChain) -> io::Result<Arc<Self>> {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<PendingRecord>(1024);
        let worker = std::thread::Builder::new()
            .name("hv2-access-audit".into())
            .spawn(move || {
                let mut failed = false;
                while let Some(first) = receiver.blocking_recv() {
                    let mut events = vec![first.event];
                    let mut completions = vec![first.done];
                    // Coalesce only work already queued. There is no extra timer
                    // delay for a lone caller. The queue backpressures at 1024.
                    while events.len() < 64 {
                        match receiver.try_recv() {
                            Ok(next) => {
                                events.push(next.event);
                                completions.push(next.done);
                            }
                            Err(_) => break,
                        }
                    }
                    let success =
                        !failed && chain.append_batch("control-plane-access", events).is_ok();
                    failed |= !success;
                    for done in completions {
                        let _ = done.send(success);
                    }
                }
            })?;
        Ok(Arc::new(Self {
            sender: Some(sender),
            worker: Some(worker),
        }))
    }

    #[cfg(test)]
    pub(crate) fn with_test_chain(chain: AuditChain) -> Arc<Self> {
        Self::with_chain(chain).expect("test audit worker")
    }

    pub fn open(path: &Path, key: [u8; 32]) -> io::Result<Arc<Self>> {
        Self::with_chain(AuditChain::open_exclusive_file(path, key)?)
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
        let (done, completion) = tokio::sync::oneshot::channel();
        self.sender
            .as_ref()
            .expect("live audit sender")
            .send(PendingRecord { event, done })
            .await
            .map_err(|_| io::Error::other("access audit unavailable"))?;
        match completion.await {
            Ok(true) => Ok(()),
            _ => Err(io::Error::other("access audit unavailable")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_core::security::audit_chain::AuditSink;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingSink(Arc<AtomicUsize>);
    impl AuditSink for FailingSink {
        fn write_line(&mut self, _: &str) -> io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::other(
                "injected failure after an uncertain partial write",
            ))
        }
    }

    struct GatedSink {
        started: tokio::sync::mpsc::UnboundedSender<usize>,
        release: std::sync::mpsc::Receiver<()>,
        lines: Arc<parking_lot::Mutex<Vec<String>>>,
    }
    impl AuditSink for GatedSink {
        fn write_line(&mut self, line: &str) -> io::Result<()> {
            self.write_lines(&[line.to_owned()])
        }
        fn write_lines(&mut self, lines: &[String]) -> io::Result<()> {
            self.lines.lock().extend_from_slice(lines);
            self.started.send(lines.len()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn queued_records_share_a_write_and_nobody_is_acknowledged_before_it_finishes() {
        use hv2_core::security::audit_chain::verify;
        let (started, mut writing) = tokio::sync::mpsc::unbounded_channel();
        let (release, gate) = std::sync::mpsc::channel();
        let lines = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let audit = AccessAudit::with_test_chain(AuditChain::new(
            [42; 32],
            Box::new(GatedSink {
                started,
                release: gate,
                lines: lines.clone(),
            }),
        ));
        let caller = audit.clone();
        let mut first = tokio::spawn(async move { caller.append(json!({"n":0})).await });
        assert_eq!(writing.recv().await, Some(1));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut first)
                .await
                .is_err()
        );
        let mut completions = Vec::new();
        for n in 1..=10 {
            let (done, completion) = tokio::sync::oneshot::channel();
            audit
                .sender
                .as_ref()
                .unwrap()
                .send(PendingRecord {
                    event: json!({"n":n}),
                    done,
                })
                .await
                .unwrap();
            completions.push(completion);
        }
        release.send(()).unwrap();
        first.await.unwrap().unwrap();
        assert_eq!(writing.recv().await, Some(10));
        for completion in &mut completions {
            assert!(matches!(
                completion.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
        }
        release.send(()).unwrap();
        for completion in completions {
            assert!(completion.await.unwrap());
        }
        drop(audit);
        let lines = lines.lock();
        assert_eq!(verify(lines.iter().map(String::as_str), &[42; 32]), Ok(11));
    }

    #[tokio::test]
    async fn failed_write_permanently_stops_all_queued_and_future_appends() {
        let calls = Arc::new(AtomicUsize::new(0));
        let audit = AccessAudit::with_test_chain(AuditChain::new(
            [42; 32],
            Box::new(FailingSink(calls.clone())),
        ));
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
