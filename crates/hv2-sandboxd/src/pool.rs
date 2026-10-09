//! A pool of sandboxes restored before anyone asked for them.
//!
//! Most of what a create waits for is the same for every sandbox of a
//! template: restore the template's guest, and wait for its agent to answer
//! and take a fresh clock and random seed. Measured on this node, that is all
//! but a millisecond or so of a create. None of it depends on who is asking.
//!
//! With `--warm-pool N` the node keeps N such guests ready: restored, their
//! agent answered and reseeded, and then put in standby, where they use no
//! CPU. A create of the base template takes one, wakes it and gives it what is
//! its own -- an ID, a token, its network, mounts and environment. Another is
//! restored in its place in the background.
//!
//! # What a spare is and is not
//!
//! - Each spare is restored and reseeded on its own, so two sandboxes handed
//!   out from the pool share no random state, exactly as two created without
//!   it do not.
//! - A spare holds its guest's memory, over and above `--capacity`: a node
//!   with a pool of N can hold N more guests than it admits.
//! - Only the base template has spares, and only a plain create takes one. A
//!   create from another template or a snapshot, a resume, a fork, and a
//!   sandbox with a disk (which cold-boots) all take the path they did.
//! - When the pool is empty a create restores a guest as before. The pool
//!   changes how long a create takes, never whether it works.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hv2_agent::AgentVM;

use super::{new_vm, sizes_of, AppState, NetDevice, Sizes, StartupVmCleanup};

/// The template spares are restored from.
pub(crate) const TEMPLATE: &str = "base";

/// A guest restored and answering, waiting in standby for an identity.
pub(crate) struct Spare {
    pub(crate) vm: Arc<AgentVM>,
    pub(crate) nic: Option<NetDevice>,
    pub(crate) cleanup: StartupVmCleanup,
    /// The snapshot it was restored from: a spare of a template that has
    /// since been replaced is not handed out.
    snapshot: PathBuf,
    with_nic: bool,
}

#[derive(Default)]
pub(crate) struct Pool {
    spares: parking_lot::Mutex<VecDeque<Spare>>,
    refill: tokio::sync::Notify,
    /// Creates that took a spare.
    handed_out: AtomicU64,
    /// Creates a spare would have served, had one been ready.
    missed: AtomicU64,
}

impl Pool {
    /// How many spares are ready, how many were handed out, and how many
    /// creates found the pool empty.
    pub(crate) fn counts(&self) -> (usize, u64, u64) {
        (
            self.spares.lock().len(),
            self.handed_out.load(Ordering::Relaxed),
            self.missed.load(Ordering::Relaxed),
        )
    }
}

/// A spare for a plain create of `template_id`, awake, if one is ready.
pub(crate) async fn take(state: &AppState, template_id: &str, with_nic: bool) -> Option<Spare> {
    if state.opts.warm_pool == 0 || template_id != TEMPLATE {
        return None;
    }
    let current = state
        .templates
        .read()
        .get(TEMPLATE)
        .map(|template| template.snapshot.clone());
    loop {
        let spare = state.pool.spares.lock().pop_front();
        state.pool.refill.notify_one();
        let Some(spare) = spare else {
            state.pool.missed.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        // Restored from a template since replaced, or without the device this
        // sandbox needs: dropped, which stops it, and the next one tried.
        if current.as_ref() != Some(&spare.snapshot) || spare.with_nic != with_nic {
            continue;
        }
        if let Err(e) = spare.vm.vm().wake().await {
            tracing::warn!("a spare sandbox could not be woken: {e}");
            continue;
        }
        state.pool.handed_out.fetch_add(1, Ordering::Relaxed);
        return Some(spare);
    }
}

/// Restore one guest of the base template and leave it in standby.
async fn warm(state: &Arc<AppState>, number: u64) -> Result<Spare, String> {
    let template = state
        .templates
        .read()
        .get(TEMPLATE)
        .cloned()
        .ok_or("the base template is not ready")?;
    let initrd = state
        .initrds
        .read()
        .get(TEMPLATE)
        .cloned()
        .unwrap_or_else(|| state.opts.initrd.clone());
    let sizes = sizes_of(state, TEMPLATE);
    let sized;
    let opts = if sizes == Sizes::of(&state.opts) {
        &state.opts
    } else {
        sized = sizes.applied(&state.opts);
        &sized
    };
    let with_nic = state.opts.network;
    let (vm, nic) = new_vm(
        opts,
        Some(&initrd),
        &format!("spare-{number}"),
        super::TEMPLATE_CID,
        with_nic.then_some(super::TEMPLATE_MAC),
        None,
    )
    .await?;
    let vm = Arc::new(vm);
    let mut cleanup = StartupVmCleanup::new(Arc::clone(&vm));
    if let Err(e) = vm
        .launch_from_snapshot_prefaulted(&template.snapshot, &template.working_set)
        .await
    {
        cleanup.stop().await;
        return Err(format!("launching: {e}"));
    }
    // Its own clock and random seed now, so the sandbox it becomes shares
    // neither with its siblings; and the proof its agent answers.
    if let Err(e) = vm.after_restore(state.opts.ready_timeout).await {
        cleanup.stop().await;
        return Err(format!("the guest never became ready: {e}"));
    }
    if let Err(e) = vm.standby().await {
        cleanup.stop().await;
        return Err(format!("standby: {e}"));
    }
    Ok(Spare {
        vm,
        nic,
        cleanup,
        snapshot: template.snapshot.clone(),
        with_nic,
    })
}

/// Keep `--warm-pool` spares ready for as long as the node runs.
pub(crate) async fn keep_full(state: Arc<AppState>) {
    let target = state.opts.warm_pool;
    if target == 0 {
        return;
    }
    let mut number = 0u64;
    loop {
        while state.pool.spares.lock().len() < target {
            number += 1;
            match warm(&state, number).await {
                Ok(spare) => state.pool.spares.lock().push_back(spare),
                Err(e) => {
                    // The template may not exist yet, or the node may be out
                    // of memory: try again, without spinning.
                    tracing::debug!("warming a spare sandbox: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        // A take asks for a refill; the timeout covers a spare that was
        // dropped as stale without anyone asking.
        let _ = tokio::time::timeout(Duration::from_secs(5), state.pool.refill.notified()).await;
    }
}
