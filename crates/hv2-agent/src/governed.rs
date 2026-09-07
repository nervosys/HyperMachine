//! Quotas and rate limits, enforced at the point every VM operation crosses.
//!
//! [`AgentPolicy`](crate::policies::AgentPolicy) has carried `quotas` and
//! `rate_limits` for a long time and neither did anything: `AgentPolicy::allows`
//! consults `enabled` and `permissions`, so a sixth VM was created under a
//! `max_vms` of five and nothing anywhere constructed a `QuotaExceeded`. The
//! machinery to count was not the missing part — [`RateLimiter`] has been in
//! `limits.rs` all along — the missing part was a place to put the count where
//! nothing could go around it.
//!
//! # Why a wrapper and not a check
//!
//! Because a check is advice. `hv2-swarm` was built the other way round
//! deliberately: `Swarm::send` is the only way a message moves, and it consults
//! the graph *inside itself*, so a caller cannot forget to ask. This is the
//! same shape for VMs. [`GovernedVmHost`] is a [`VmHost`] that wraps another
//! one, and an operation it refuses never reaches the inner host at all —
//! which is the thing worth asserting, and what the tests below assert.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use std::time::Duration;
//! # use hv2_agent::governed::GovernedVmHost;
//! # use hv2_agent::policies::{PolicyAction, QuotaSpec, RateLimitSpec};
//! # use hv2_agent::vm_host::LocalVmHost;
//! let host = GovernedVmHost::new(LocalVmHost::new(), QuotaSpec::basic())
//!     .with_rate_limit(PolicyAction::VmCreate, RateLimitSpec::per_minute(10));
//! // Hand this to the MCP server instead of the bare host.
//! let host: Arc<dyn hv2_agent::VmHost> = Arc::new(host);
//! ```
//!
//! # Where the counters live, which was the open question
//!
//! In the wrapper, and therefore in one process, for the lifetime of the
//! wrapper. Three consequences, stated rather than discovered:
//!
//! - **Per agent or per session is decided by how many you build.** The
//!   [`VmHost`] trait carries no agent id, deliberately: the MCP server has
//!   already established that the calling session owns the VM before it
//!   dispatches, so identity is settled above this layer. One governor per
//!   session governs a session; one wrapping a shared host governs everything
//!   through it.
//! - **Quotas are released on `delete`, not on `stop`.** A stopped VM still
//!   holds its memory reservation and its name; a deleted one does not. The
//!   cost of each admitted VM is remembered so that deleting it returns
//!   exactly what creating it took, rather than a re-derived guess.
//! - **Nothing survives a restart.** A process that comes back up has admitted
//!   nothing and will admit a full quota again. Persisting usage needs a store
//!   and an answer to what happens when the store disagrees with the host's
//!   inventory, which is a larger decision than this; what is here is honest
//!   in-process enforcement, and it is enforcement rather than description.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::limits::RateLimiter;
use crate::policies::{PolicyAction, QuotaSpec, RateLimitSpec};
use crate::vm_host::{GuestCommand, VmDescriptor, VmExec, VmHost, VmSpec};

/// What one admitted VM costs against the quota.
///
/// Remembered per VM rather than recomputed, because a VM's spec is the
/// caller's and a caller that asks for one thing and is billed another is a
/// bug that only shows up as a quota drifting over hours.
#[derive(Debug, Clone, Copy)]
struct VmCost {
    memory: u64,
    cpus: u32,
}

impl VmCost {
    fn of(spec: &VmSpec) -> Self {
        Self {
            memory: spec.memory_gb.saturating_mul(1024 * 1024 * 1024),
            cpus: spec.cpu_cores,
        }
    }
}

/// Everything the governor counts.
#[derive(Debug, Default)]
struct Usage {
    /// Cost of each VM this governor admitted, by id.
    admitted: HashMap<String, VmCost>,
    memory: u64,
    cpus: u32,
}

impl Usage {
    fn vms(&self) -> u32 {
        self.admitted.len() as u32
    }

    fn record(&mut self, vm_id: String, cost: VmCost) {
        self.memory = self.memory.saturating_add(cost.memory);
        self.cpus = self.cpus.saturating_add(cost.cpus);
        self.admitted.insert(vm_id, cost);
    }

    /// Return what a VM took, if this governor is the one that admitted it.
    ///
    /// A VM it never admitted releases nothing. That case is real -- a host
    /// shared between governors, or a VM created before this one existed -- and
    /// subtracting for it would drive the tally negative and hand out quota
    /// that was never returned.
    fn release(&mut self, vm_id: &str) {
        if let Some(cost) = self.admitted.remove(vm_id) {
            self.memory = self.memory.saturating_sub(cost.memory);
            self.cpus = self.cpus.saturating_sub(cost.cpus);
        }
    }
}

/// A [`VmHost`] that enforces quotas and rate limits before delegating.
///
/// See the module documentation for why this is a wrapper rather than a check,
/// and for what the counters do and do not survive.
pub struct GovernedVmHost<H> {
    inner: H,
    quotas: QuotaSpec,
    limiters: HashMap<PolicyAction, RateLimiter>,
    usage: Mutex<Usage>,
}

impl<H> GovernedVmHost<H> {
    /// Govern `inner` with `quotas` and no rate limits.
    pub fn new(inner: H, quotas: QuotaSpec) -> Self {
        Self {
            inner,
            quotas,
            limiters: HashMap::new(),
            usage: Mutex::new(Usage::default()),
        }
    }

    /// Rate-limit one action.
    ///
    /// Actions with no limit are not rate limited, which is why this is opt-in
    /// per action rather than a single number: creating a VM and reading its
    /// status are the same kind of call to this layer and nothing like the same
    /// cost to the host.
    #[must_use]
    pub fn with_rate_limit(mut self, action: PolicyAction, limit: RateLimitSpec) -> Self {
        self.limiters
            .insert(action, RateLimiter::new(limit.max_operations, limit.window));
        self
    }

    /// Govern `inner` with everything an [`AgentPolicy`](crate::policies::AgentPolicy)
    /// records: its quotas and every rate limit it names.
    ///
    /// This is the whole path from a policy to enforcement, and it is one call
    /// because the previous distance between them was the defect. A policy's
    /// `quotas` and `rate_limits` were data that nothing read; handing the
    /// policy here makes them the thing that decides.
    ///
    /// Permissions are deliberately *not* consulted. They are checked above
    /// this layer, against an action and a resource this trait does not carry,
    /// and re-deciding them here with less information than the caller had is
    /// how two answers to the same question start disagreeing.
    pub fn from_policy(inner: H, policy: &crate::policies::AgentPolicy) -> Self {
        let mut host = Self::new(inner, policy.quotas.clone());
        for (action, limit) in &policy.rate_limits {
            host = host.with_rate_limit(action.clone(), limit.clone());
        }
        host
    }

    /// The host being governed.
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// How many VMs this governor has admitted and not seen deleted.
    pub fn admitted_vms(&self) -> u32 {
        self.usage.lock().unwrap_or_else(|e| e.into_inner()).vms()
    }

    /// Guest memory, in bytes, across every VM this governor admitted.
    pub fn admitted_memory(&self) -> u64 {
        self.usage.lock().unwrap_or_else(|e| e.into_inner()).memory
    }

    /// vCPUs across every VM this governor admitted.
    pub fn admitted_cpus(&self) -> u32 {
        self.usage.lock().unwrap_or_else(|e| e.into_inner()).cpus
    }

    /// Consume one permit for `action`, or say why not.
    ///
    /// The permit is taken here, before the operation runs. An operation that
    /// then fails for its own reasons has still spent one, which is the right
    /// way round: a rate limit exists to bound how often something is *asked
    /// for*, and a caller whose failures were free could drive the host as hard
    /// as it liked by asking for things that do not work.
    fn take_permit(&self, action: PolicyAction) -> Result<(), String> {
        let Some(limiter) = self.limiters.get(&action) else {
            return Ok(());
        };
        limiter
            .try_acquire()
            .map_err(|e| format!("rate limit: {e}"))
    }

    /// Check `cost` against every quota, without recording anything.
    fn admits(&self, cost: VmCost, usage: &Usage) -> Result<(), String> {
        if !self.quotas.allows_vm(usage.vms()) {
            return Err(format!(
                "quota exceeded: this agent holds {} VMs and its limit is {}",
                usage.vms(),
                self.quotas.max_vms.unwrap_or(u32::MAX)
            ));
        }
        if !self.quotas.allows_memory(usage.memory, cost.memory) {
            return Err(format!(
                "quota exceeded: this agent holds {} bytes of guest memory and another {} \
                 would pass its limit of {}",
                usage.memory,
                cost.memory,
                self.quotas.max_memory.unwrap_or(u64::MAX)
            ));
        }
        if !self.quotas.allows_cpus(usage.cpus, cost.cpus) {
            return Err(format!(
                "quota exceeded: this agent holds {} vCPUs and another {} would pass its \
                 limit of {}",
                usage.cpus,
                cost.cpus,
                self.quotas.max_cpus.unwrap_or(u32::MAX)
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl<H> VmHost for GovernedVmHost<H>
where
    H: VmHost,
{
    async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::VmCreate)?;
        let cost = VmCost::of(&spec);

        // Checked and reserved under one lock, so two concurrent creates cannot
        // both see room for the last VM. Reserving before delegating is what
        // closes that window: leaving the tally alone until the inner host
        // answers would let every concurrent create pass the same check.
        //
        // The reservation is held under a unique placeholder id, because the VM
        // has no id of its own until the inner host returns one.
        let reservation = reservation_id(&spec);
        {
            let mut usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
            self.admits(cost, &usage)?;
            usage.record(reservation.clone(), cost);
        }

        // The lock is not held across this await. A `std::sync::Mutex` guard
        // held over one would block the executor rather than yield it.
        let created = self.inner.create(spec).await;

        let mut usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.release(&reservation);
        let descriptor = created?;
        usage.record(descriptor.vm_id.clone(), cost);
        Ok(descriptor)
    }

    async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::VmStart)?;
        self.inner.start(vm_id).await
    }

    async fn stop(&self, vm_id: &str, force: bool) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::VmStop)?;
        // No release here. A stopped VM still holds its memory reservation and
        // its id; only `delete` gives those back.
        self.inner.stop(vm_id, force).await
    }

    async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::VmPause)?;
        self.inner.pause(vm_id).await
    }

    async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::VmResume)?;
        self.inner.resume(vm_id).await
    }

    async fn delete(&self, vm_id: &str) -> Result<(), String> {
        self.take_permit(PolicyAction::VmDelete)?;
        // Released only once the inner host confirms. A delete that failed
        // leaves the VM running, and returning its quota would let the agent
        // create a replacement it has no room for.
        self.inner.delete(vm_id).await?;
        self.usage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .release(vm_id);
        Ok(())
    }

    async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.take_permit(PolicyAction::ResourceRead)?;
        self.inner.status(vm_id).await
    }

    async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
        self.take_permit(PolicyAction::ResourceRead)?;
        self.inner.list().await
    }

    async fn exec(&self, vm_id: &str, command: GuestCommand) -> Result<VmExec, String> {
        self.take_permit(PolicyAction::GuestExec)?;
        self.inner.exec(vm_id, command).await
    }
}

/// A placeholder key for a reservation held while the inner host is creating.
///
/// Not a random id: two identical concurrent creates should each hold their own
/// reservation, and a map keyed by spec would collapse them into one. The
/// counter makes each unique.
fn reservation_id(spec: &VmSpec) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "reserving:{}:{}",
        spec.name,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A host that does nothing but count what reached it.
    ///
    /// Counting is the whole point. A governor that refuses correctly and
    /// delegates anyway is indistinguishable, from the caller's side, from one
    /// that works -- right up until the host runs out of memory. So every test
    /// here asserts on what the inner host saw, not on what the governor
    /// returned.
    #[derive(Default)]
    struct CountingHost {
        creates: AtomicUsize,
        deletes: AtomicUsize,
        starts: AtomicUsize,
        next_id: AtomicUsize,
    }

    #[async_trait]
    impl VmHost for CountingHost {
        async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            let n = self.next_id.fetch_add(1, Ordering::SeqCst);
            Ok(descriptor(
                &format!("vm-{n}"),
                &spec.name,
                spec.cpu_cores,
                spec.memory_gb,
            ))
        }
        async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.status(vm_id).await
        }
        async fn stop(&self, vm_id: &str, _force: bool) -> Result<VmDescriptor, String> {
            self.status(vm_id).await
        }
        async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String> {
            self.status(vm_id).await
        }
        async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String> {
            self.status(vm_id).await
        }
        async fn delete(&self, _vm_id: &str) -> Result<(), String> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
            Ok(descriptor(vm_id, vm_id, 1, 1))
        }
        async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
            Ok(Vec::new())
        }
    }

    /// A host that refuses everything, for the paths where what matters is
    /// what the governor does with a failure.
    #[derive(Default)]
    struct RefusingHost {
        deletes: AtomicUsize,
    }

    #[async_trait]
    impl VmHost for RefusingHost {
        async fn create(&self, _spec: VmSpec) -> Result<VmDescriptor, String> {
            Err("the inner host refused".to_string())
        }
        async fn start(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            Err("no".to_string())
        }
        async fn stop(&self, _vm_id: &str, _force: bool) -> Result<VmDescriptor, String> {
            Err("no".to_string())
        }
        async fn pause(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            Err("no".to_string())
        }
        async fn resume(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            Err("no".to_string())
        }
        async fn delete(&self, _vm_id: &str) -> Result<(), String> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            Err("the inner host refused".to_string())
        }
        async fn status(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            Err("no".to_string())
        }
        async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
            Err("no".to_string())
        }
    }

    fn spec(name: &str, cpus: u32, memory_gb: u64) -> VmSpec {
        VmSpec {
            name: name.to_string(),
            cpu_cores: cpus,
            memory_gb,
            enable_gpu: false,
            enable_networking: false,
            boot: None,
            guest_cid: None,
        }
    }

    fn descriptor(vm_id: &str, name: &str, cpu_cores: u32, memory_gb: u64) -> VmDescriptor {
        VmDescriptor {
            vm_id: vm_id.to_string(),
            name: name.to_string(),
            cpu_cores,
            memory_gb,
            status: "created".to_string(),
            boot_protocol: None,
        }
    }

    /// Two VMs under a limit of two, and the third does not reach the host.
    #[tokio::test]
    async fn a_vm_over_the_quota_never_reaches_the_host() {
        let quotas = QuotaSpec {
            max_vms: Some(2),
            ..QuotaSpec::unlimited()
        };
        let host = GovernedVmHost::new(CountingHost::default(), quotas);

        assert!(host.create(spec("a", 1, 1)).await.is_ok());
        assert!(host.create(spec("b", 1, 1)).await.is_ok());

        let refused = host.create(spec("c", 1, 1)).await;
        let message = refused.expect_err("a third VM is over the limit of two");
        assert!(
            message.contains("quota exceeded"),
            "the refusal should say why: {message}"
        );

        assert_eq!(
            host.inner().creates.load(Ordering::SeqCst),
            2,
            "the refused create must not have reached the inner host -- a governor that \\
             refuses and delegates anyway is a governor that does nothing"
        );
        assert_eq!(host.admitted_vms(), 2);
    }

    /// Memory is counted in bytes from a spec in gigabytes, and the sum is what
    /// the limit applies to rather than any single request.
    #[tokio::test]
    async fn memory_is_summed_across_vms() {
        let quotas = QuotaSpec {
            max_memory: Some(4 * 1024 * 1024 * 1024),
            ..QuotaSpec::unlimited()
        };
        let host = GovernedVmHost::new(CountingHost::default(), quotas);

        assert!(host.create(spec("a", 1, 3)).await.is_ok());
        assert_eq!(host.admitted_memory(), 3 * 1024 * 1024 * 1024);

        // 3 + 2 is over 4, though neither is over on its own.
        let refused = host.create(spec("b", 1, 2)).await;
        assert!(refused.is_err(), "3 GiB plus 2 GiB passes a 4 GiB limit");
        assert_eq!(host.inner().creates.load(Ordering::SeqCst), 1);
    }

    /// Deleting returns exactly what creating took.
    #[tokio::test]
    async fn deleting_returns_the_quota_the_vm_took() {
        let quotas = QuotaSpec {
            max_vms: Some(1),
            max_memory: Some(8 * 1024 * 1024 * 1024),
            max_cpus: Some(4),
            ..QuotaSpec::unlimited()
        };
        let host = GovernedVmHost::new(CountingHost::default(), quotas);

        let first = host.create(spec("a", 4, 8)).await.expect("first VM");
        assert!(
            host.create(spec("b", 1, 1)).await.is_err(),
            "the quota is full"
        );

        host.delete(&first.vm_id).await.expect("delete");
        assert_eq!(host.admitted_vms(), 0);
        assert_eq!(host.admitted_memory(), 0);
        assert_eq!(host.admitted_cpus(), 0);

        assert!(
            host.create(spec("b", 4, 8)).await.is_ok(),
            "the room the deleted VM held should be available again"
        );
    }

    /// Stopping is not deleting. This is the distinction most likely to be got
    /// wrong, and getting it wrong hands out memory the host has not freed.
    #[tokio::test]
    async fn stopping_a_vm_does_not_return_its_quota() {
        let quotas = QuotaSpec {
            max_vms: Some(1),
            ..QuotaSpec::unlimited()
        };
        let host = GovernedVmHost::new(CountingHost::default(), quotas);

        let vm = host.create(spec("a", 1, 1)).await.expect("first VM");
        host.stop(&vm.vm_id, false).await.expect("stop");

        assert_eq!(host.admitted_vms(), 1, "a stopped VM still holds its slot");
        assert!(host.create(spec("b", 1, 1)).await.is_err());
    }

    /// A create the inner host refused costs nothing.
    #[tokio::test]
    async fn a_failed_create_holds_no_quota() {
        let quotas = QuotaSpec {
            max_vms: Some(1),
            ..QuotaSpec::unlimited()
        };
        let host = GovernedVmHost::new(RefusingHost::default(), quotas);

        assert!(host.create(spec("a", 1, 1)).await.is_err());
        assert_eq!(
            host.admitted_vms(),
            0,
            "the reservation taken before delegating must be released when the host refuses"
        );
    }

    /// A delete the inner host refused keeps the quota held.
    #[tokio::test]
    async fn a_failed_delete_keeps_the_quota_held() {
        let host = GovernedVmHost::new(RefusingHost::default(), QuotaSpec::unlimited());

        assert!(host.delete("vm-0").await.is_err());
        assert_eq!(
            host.inner().deletes.load(Ordering::SeqCst),
            1,
            "the delete should have been attempted"
        );
        // Nothing was admitted through this governor, so there is nothing to
        // check but that it did not invent a release. The interesting case is
        // covered by `deleting_returns_the_quota_the_vm_took`, which only
        // releases on success.
    }

    /// The rate limit refuses, and the refused call does not reach the host.
    #[tokio::test]
    async fn a_rate_limited_call_never_reaches_the_host() {
        let host = GovernedVmHost::new(CountingHost::default(), QuotaSpec::unlimited())
            .with_rate_limit(
                PolicyAction::VmStart,
                RateLimitSpec::new(2, Duration::from_secs(60)),
            );

        assert!(host.start("vm-0").await.is_ok());
        assert!(host.start("vm-0").await.is_ok());

        let refused = host.start("vm-0").await;
        let message = refused.expect_err("a third start inside the window is over the limit");
        assert!(
            message.contains("rate limit"),
            "the refusal should say why: {message}"
        );
        assert_eq!(
            host.inner().starts.load(Ordering::SeqCst),
            2,
            "the refused start must not have reached the inner host"
        );
    }

    /// An action with no limit configured is not rate limited.
    #[tokio::test]
    async fn an_action_without_a_limit_is_not_limited() {
        let host = GovernedVmHost::new(CountingHost::default(), QuotaSpec::unlimited())
            .with_rate_limit(
                PolicyAction::VmCreate,
                RateLimitSpec::new(1, Duration::from_secs(60)),
            );

        for _ in 0..10 {
            assert!(host.start("vm-0").await.is_ok());
        }
        assert_eq!(host.inner().starts.load(Ordering::SeqCst), 10);
    }

    /// A policy's own quotas and rate limits are what get enforced, which is
    /// the whole point: the fields existed and nothing read them.
    #[tokio::test]
    async fn a_policy_is_what_the_governor_enforces() {
        let mut policy = crate::policies::AgentPolicy::new("p", "three VMs");
        policy.quotas = QuotaSpec {
            max_vms: Some(3),
            ..QuotaSpec::unlimited()
        };
        policy.rate_limits.insert(
            PolicyAction::VmStart,
            RateLimitSpec::new(1, Duration::from_secs(60)),
        );

        let host = GovernedVmHost::from_policy(CountingHost::default(), &policy);

        for i in 0..3 {
            assert!(host.create(spec(&format!("vm-{i}"), 1, 1)).await.is_ok());
        }
        assert!(host.create(spec("vm-3", 1, 1)).await.is_err());
        assert_eq!(host.inner().creates.load(Ordering::SeqCst), 3);

        assert!(host.start("vm-0").await.is_ok());
        assert!(host.start("vm-0").await.is_err());
        assert_eq!(host.inner().starts.load(Ordering::SeqCst), 1);
    }

    /// Unlimited quotas admit everything, which is what an unconfigured
    /// governor must do: wrapping a host should not change its behaviour until
    /// someone sets a limit.
    #[tokio::test]
    async fn an_ungoverned_wrapper_changes_nothing() {
        let host = GovernedVmHost::new(CountingHost::default(), QuotaSpec::unlimited());

        for i in 0..20 {
            assert!(host.create(spec(&format!("vm-{i}"), 8, 64)).await.is_ok());
        }
        assert_eq!(host.inner().creates.load(Ordering::SeqCst), 20);
    }
}
