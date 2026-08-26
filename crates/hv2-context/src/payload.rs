//! Where large payloads go, and what stays behind in their place.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ContextError, Result};

/// Payloads at or above this many bytes are written to the store instead of
/// being kept inline in the log.
///
/// The number is a tradeoff, not a law: below it, a payload costs one line in
/// a file that gets scanned; above it, that scan starts to cost more than the
/// extra open. 8 KiB is roughly where a tool result stops being a message and
/// starts being a document.
pub const EXTERNALIZE_THRESHOLD: usize = 8 * 1024;

/// How much of an externalized payload stays visible in the log.
///
/// Not decoration. Without it, a scan of the log shows a row of handles and no
/// indication of what any of them are, and the only way to tell them apart is
/// to open every one.
pub const PREVIEW_BYTES: usize = 240;

/// A reference to bytes held outside the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handle {
    /// Identifier, and the file name under the store's root.
    pub id: String,
    /// Length of the stored bytes.
    pub bytes: u64,
}

/// An event's content: either the text itself, or where to find it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum Payload {
    /// Small enough to keep in the log.
    Inline {
        /// The content.
        text: String,
    },
    /// Held in the [`PayloadStore`]; the log keeps a preview and a handle.
    External {
        /// Where the bytes are.
        handle: Handle,
        /// The opening of the content, so the log stays readable.
        preview: String,
    },
}

impl Payload {
    /// The text if it is inline, the preview if it is not.
    ///
    /// Deliberately never reads a file: this is what a *scan* of the log can
    /// afford to look at. Use [`PayloadStore::materialize`] for the whole
    /// thing, which is the operation an agent chooses to pay for.
    pub fn visible_text(&self) -> &str {
        match self {
            Self::Inline { text } => text,
            Self::External { preview, .. } => preview,
        }
    }

    /// Whether the whole content is present without touching the store.
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// Length of the content, whether or not it is inline.
    pub fn len(&self) -> u64 {
        match self {
            Self::Inline { text } => text.len() as u64,
            Self::External { handle, .. } => handle.bytes,
        }
    }

    /// Whether the content is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Bytes that were too large for the log, kept on disk under one directory.
#[derive(Debug, Clone)]
pub struct PayloadStore {
    root: PathBuf,
    threshold: usize,
}

impl PayloadStore {
    /// Open (creating if needed) a store rooted at `root`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Io`] if the directory cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|e| ContextError::Io(format!("creating {}: {e}", root.display())))?;
        Ok(Self {
            root,
            threshold: EXTERNALIZE_THRESHOLD,
        })
    }

    /// The same store with a different externalization threshold.
    ///
    /// Exists for tests, which would otherwise have to build 8 KiB of text to
    /// exercise the path that matters.
    #[must_use]
    pub fn with_threshold(mut self, bytes: usize) -> Self {
        self.threshold = bytes;
        self
    }

    /// Where this store keeps its files.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Turn text into a payload, externalizing it if it is large.
    ///
    /// `seq_hint` only names the file; correctness does not depend on it being
    /// the sequence number the event eventually gets.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::Io`] if the payload file cannot be written.
    pub fn store(&self, seq_hint: u64, text: &str) -> Result<Payload> {
        if text.len() < self.threshold {
            return Ok(Payload::Inline {
                text: text.to_string(),
            });
        }

        let id = format!("{seq_hint:012}.txt");
        let path = self.root.join(&id);
        std::fs::write(&path, text.as_bytes())
            .map_err(|e| ContextError::Io(format!("writing {}: {e}", path.display())))?;

        Ok(Payload::External {
            handle: Handle {
                id,
                bytes: text.len() as u64,
            },
            preview: truncate_on_char_boundary(text, PREVIEW_BYTES),
        })
    }

    /// Read a payload back in full.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::LostPayload`] if the bytes behind a handle are
    /// gone. The event itself is unaffected, which is why this is a distinct
    /// error rather than a missing event.
    pub fn materialize(&self, payload: &Payload) -> Result<String> {
        match payload {
            Payload::Inline { text } => Ok(text.clone()),
            Payload::External { handle, .. } => {
                let path = self.root.join(&handle.id);
                std::fs::read_to_string(&path)
                    .map_err(|e| ContextError::LostPayload(handle.id.clone(), e.to_string()))
            }
        }
    }
}

/// Cut `text` to at most `bytes`, never through the middle of a character.
///
/// Shared with the search index, which has to bound previews for inline
/// payloads too: those keep their whole content in the log, so a hit that
/// echoed `visible_text` would return the entire payload for every match.
///
/// Slicing a `String` by byte index panics on a multi-byte boundary, and a
/// preview is exactly where arbitrary tool output meets an arbitrary cut.
pub(crate) fn truncate_on_char_boundary(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_string();
    }
    let mut end = bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, PayloadStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = PayloadStore::open(dir.path().join("payloads")).unwrap();
        (dir, store)
    }

    #[test]
    fn small_payloads_stay_in_the_log() {
        let (_dir, store) = store();
        let payload = store.store(1, "a short tool result").unwrap();
        assert!(payload.is_inline());
        assert_eq!(payload.visible_text(), "a short tool result");
        assert_eq!(store.materialize(&payload).unwrap(), "a short tool result");
    }

    #[test]
    fn large_payloads_leave_a_preview_and_come_back_whole() {
        let (_dir, store) = store();
        let big = "x".repeat(EXTERNALIZE_THRESHOLD + 500);
        let payload = store.store(7, &big).unwrap();

        assert!(!payload.is_inline());
        assert_eq!(
            payload.visible_text().len(),
            PREVIEW_BYTES,
            "a scan of the log has to be able to see what this is"
        );
        assert_eq!(payload.len(), big.len() as u64);
        assert_eq!(
            store.materialize(&payload).unwrap(),
            big,
            "externalizing is not a lossy operation"
        );
    }

    #[test]
    fn a_preview_never_cuts_through_a_character() {
        let (_dir, store) = store();
        // Each of these is three bytes, so a cut at PREVIEW_BYTES lands mid
        // character unless the boundary is respected. Slicing there panics.
        let text = "\u{65e5}".repeat(EXTERNALIZE_THRESHOLD);
        let payload = store.store(1, &text).unwrap();
        let preview = payload.visible_text();
        assert!(preview.len() <= PREVIEW_BYTES);
        assert!(text.starts_with(preview));
    }

    #[test]
    fn a_lost_payload_is_reported_as_itself() {
        let (_dir, store) = store();
        let payload = store
            .clone()
            .with_threshold(1)
            .store(3, "externalize me")
            .unwrap();
        let Payload::External { ref handle, .. } = payload else {
            panic!("expected an external payload");
        };
        std::fs::remove_file(store.root().join(&handle.id)).unwrap();

        // The distinction matters: the event is still in the log and still
        // addressable, and only the bytes behind it are gone.
        let err = store.materialize(&payload).unwrap_err();
        assert!(matches!(err, ContextError::LostPayload(..)), "got: {err}");
    }
}
