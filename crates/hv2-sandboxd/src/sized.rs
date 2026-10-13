//! Templates at sizes other than their own.
//!
//! A template's snapshot is of a guest its size, and cannot be restored as
//! another; so a sandbox of another size boots, which takes hundreds of
//! milliseconds where a restore takes tens. The first sandbox of a template
//! at a size still boots. While it does, the node builds the template again
//! at that size, and every later sandbox of that template and size --
//! created, resized, forked or resumed at it -- is restored from that.
//!
//! `--sized-templates N` (default 4) bounds how many such snapshots a node
//! keeps, each as large on disk as the guest's memory; 0 makes every
//! sandbox of another size boot, as before. `--template-size CPUSxMB`
//! builds the base template at that size when the node starts.
//!
//! # What a sandbox restored from one is
//!
//! It is still a size of its own: its pause writes its memory whole, and it
//! cannot be checkpointed or made a template, as a sandbox of another size
//! that booted cannot. Only how fast it starts changes. A sandbox with a
//! disk boots, as it does at its template's size.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{build_template, AppState, Sizes, Template};

/// What a node keeps unless told otherwise.
pub(crate) const DEFAULT_LIMIT: usize = 4;

/// The sized templates this node has built, and those it is building.
#[derive(Default)]
pub(crate) struct SizedTemplates {
    built: parking_lot::RwLock<BTreeMap<String, Arc<Template>>>,
    building: parking_lot::Mutex<BTreeSet<String>>,
    /// One build at a time: each boots a guest beside the node's sandboxes.
    one_at_a_time: tokio::sync::Mutex<()>,
}

fn key(template: &str, size: Sizes) -> String {
    format!("{template}@{}x{}", size.cpus, size.memory_mb)
}

/// The snapshot of `template` at `size`, if this node has one.
pub(crate) fn get(state: &AppState, template: &str, size: Sizes) -> Option<Arc<Template>> {
    state.sized.built.read().get(&key(template, size)).cloned()
}

/// Build `template` at `size` in the background, unless it is built, being
/// built, or the node holds as many as it may. Nothing waits for it.
pub(crate) fn want(state: &Arc<AppState>, template: &str, size: Sizes) {
    if state.opts.no_template || state.opts.sized_templates == 0 {
        return;
    }
    // Only a template this node restores from at its own size.
    if !state.templates.read().contains_key(template) {
        return;
    }
    let Some(initrd) = state.initrds.read().get(template).cloned() else {
        return;
    };
    let name = key(template, size);
    {
        let built = state.sized.built.read();
        let mut building = state.sized.building.lock();
        if built.contains_key(&name)
            || building.contains(&name)
            || built.len() + building.len() >= state.opts.sized_templates
        {
            return;
        }
        building.insert(name.clone());
    }
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let _one = state.sized.one_at_a_time.lock().await;
        let mut opts = size.applied(&state.opts);
        opts.initrd = initrd.clone();
        let dir = std::env::temp_dir().join(format!(
            "hv2-sandboxd-{}-{}-{}",
            std::process::id(),
            name.replace(['@', '/'], "-"),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let started = std::time::Instant::now();
        let built = build_template(&opts, state.authority.as_deref(), dir).await;
        state.sized.building.lock().remove(&name);
        match built {
            // Kept only if the template is still the one it was built from:
            // a rebuild under the same name forgets its sizes (see `forget`).
            Ok(template)
                if state
                    .initrds
                    .read()
                    .get(&key_template(&name))
                    .map(String::as_str)
                    == Some(initrd.as_str()) =>
            {
                tracing::info!(
                    "{name}: restored from now on, built in {:?}",
                    started.elapsed()
                );
                state.sized.built.write().insert(name, Arc::new(template));
            }
            Ok(_) => tracing::info!("{name}: its template changed while it was built; dropped"),
            Err(e) => tracing::warn!("{name}: not built ({e}); sandboxes that size boot"),
        }
    });
}

/// The template a key names.
fn key_template(name: &str) -> String {
    name.rsplit_once('@').map_or(name, |(t, _)| t).to_string()
}

/// Forget every size of `template`, which was rebuilt: they are of its old
/// guest. A sandbox restored from one keeps it until it ends.
pub(crate) fn forget(state: &AppState, template: &str) {
    let prefix = format!("{template}@");
    state
        .sized
        .built
        .write()
        .retain(|name, _| !name.starts_with(&prefix));
}

/// `CPUSxMB`, as `--template-size` takes it.
pub(crate) fn parse(value: &str) -> Result<Sizes, String> {
    let (cpus, memory_mb) = value
        .split_once('x')
        .ok_or_else(|| format!("--template-size {value}: CPUSxMB, as 2x4096"))?;
    let cpus: u32 = cpus
        .parse()
        .map_err(|e| format!("--template-size {value}: {e}"))?;
    let memory_mb: u64 = memory_mb
        .parse()
        .map_err(|e| format!("--template-size {value}: {e}"))?;
    Sizes::requested(Some(cpus), Some(memory_mb), Sizes { cpus, memory_mb })
        .map_err(|e| format!("--template-size {value}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_names_its_template_and_size() {
        let size = Sizes {
            cpus: 2,
            memory_mb: 4096,
        };
        assert_eq!(key("base", size), "base@2x4096");
        assert_eq!(key_template(&key("my@odd", size)), "my@odd");
    }

    #[test]
    fn a_template_size_is_cpus_by_megabytes() {
        let size = parse("1x512").unwrap();
        assert_eq!((size.cpus, size.memory_mb), (1, 512));
        for wrong in ["2", "x4096", "2x", "0x4096", "1x100", "1x1023", "ax512"] {
            assert!(parse(wrong).is_err(), "{wrong}");
        }
    }
}
