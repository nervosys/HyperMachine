//! Runtime CLI Commands
//!
//! Fleet-level subcommands for the HyperMachine runtime — session lifecycle,
//! workload scheduling, workflow orchestration, maintenance, and status.
//!
//! ## Command Groups
//!
//! | Subcommand                          | Description                              |
//! |-------------------------------------|------------------------------------------|
//! | `runtime status`                    | Show runtime status snapshot             |
//! | `runtime session create`            | Create an agent session                  |
//! | `runtime session destroy`           | Destroy an agent session                 |
//! | `runtime workload submit`           | Submit workload for scheduling           |
//! | `runtime workload schedule`         | Batch schedule all pending workloads     |
//! | `runtime workflow run`              | Submit & start a DAG workflow            |
//! | `runtime workflow advance`          | Advance a workflow step                  |
//! | `runtime workflow cancel`           | Cancel a running workflow                |
//! | `runtime maintenance`               | Trigger maintenance tick                 |

use clap::Subcommand;
use colored::*;
use hv2_runtime::{
    BillingTier, Placement, Runtime, SessionInfo, StepOutcome, WorkflowSpec, WorkloadDescriptor,
    WorkloadResult,
};
use std::time::{Duration, SystemTime};

// ============================================================================
// CLI Subcommands
// ============================================================================

/// Runtime management subcommands
#[derive(Subcommand)]
pub enum RuntimeCommands {
    /// Show runtime status
    Status,

    /// Session management
    #[command(subcommand)]
    Session(SessionCommands),

    /// Workload management
    #[command(subcommand)]
    Workload(WorkloadCommands),

    /// Workflow orchestration
    #[command(subcommand)]
    Workflow(WorkflowCommands),

    /// Trigger maintenance tick
    Maintenance,
}

/// Session subcommands
#[derive(Subcommand)]
pub enum SessionCommands {
    /// Create a new agent session
    Create {
        /// Session ID
        #[arg(short, long)]
        id: String,

        /// Billing tier (free, standard, premium, enterprise)
        #[arg(short, long, default_value = "standard")]
        tier: String,
    },

    /// Destroy an existing session
    Destroy {
        /// Session ID
        id: String,
    },
}

/// Workload subcommands
#[derive(Subcommand)]
pub enum WorkloadCommands {
    /// Submit a workload for scheduling
    Submit {
        /// Workload ID
        #[arg(short, long)]
        id: String,

        /// Target session ID
        #[arg(short, long)]
        session: String,

        /// Required vCPUs
        #[arg(long, default_value = "1")]
        vcpus: u32,

        /// Required memory in bytes
        #[arg(long, default_value = "536870912")]
        memory: u64,

        /// Require GPU
        #[arg(long)]
        gpu: bool,

        /// Priority (0-100, higher = more urgent)
        #[arg(short, long, default_value = "50")]
        priority: u32,

        /// Placement timeout in seconds
        #[arg(long, default_value = "30")]
        timeout: u64,
    },

    /// Schedule all pending workloads
    Schedule,
}

/// Workflow subcommands
#[derive(Subcommand)]
pub enum WorkflowCommands {
    /// Run a workflow from a JSON specification
    Run {
        /// Path to workflow spec JSON file
        #[arg(short, long)]
        spec: String,
    },

    /// Advance a workflow step with an outcome
    Advance {
        /// Workflow ID
        #[arg(short, long)]
        workflow: String,

        /// Step name
        #[arg(short, long)]
        step: String,

        /// Outcome: "success", "failure:\<message\>", or "skip:\<reason\>"
        #[arg(short, long, default_value = "success")]
        outcome: String,
    },

    /// Cancel a running workflow
    Cancel {
        /// Workflow ID
        id: String,
    },
}

// ============================================================================
// Execution
// ============================================================================

/// Execute a runtime subcommand against the given runtime.
pub fn execute(runtime: &Runtime, cmd: &RuntimeCommands) -> anyhow::Result<()> {
    match cmd {
        RuntimeCommands::Status => exec_status(runtime),
        RuntimeCommands::Session(sub) => exec_session(runtime, sub),
        RuntimeCommands::Workload(sub) => exec_workload(runtime, sub),
        RuntimeCommands::Workflow(sub) => exec_workflow(runtime, sub),
        RuntimeCommands::Maintenance => exec_maintenance(runtime),
    }
}

// ── Status ────────────────────────────────────────────────────────────

fn exec_status(runtime: &Runtime) -> anyhow::Result<()> {
    let status = runtime.status();

    println!("{}", "Runtime Status".cyan().bold());
    println!("  Instance:          {}", status.instance_id.green());
    println!(
        "  Pool:              {} warm, {} busy, {} total",
        status.pool.warm.to_string().green(),
        status.pool.assigned.to_string().yellow(),
        status.pool.total.to_string().cyan(),
    );
    println!(
        "  Active routes:     {}",
        status.active_routes.to_string().cyan()
    );
    println!(
        "  Pending workloads: {}",
        status.pending_workloads.to_string().cyan()
    );
    println!(
        "  Active workflows:  {}",
        status.active_workflows.to_string().cyan()
    );
    println!(
        "  Store entries:     {}",
        status.store_entries.to_string().cyan()
    );
    println!(
        "  Billing sessions:  {}",
        status.billing_sessions.to_string().cyan()
    );

    if !status.health_summary.is_empty() {
        println!("  Health:");
        for (hs, count) in &status.health_summary {
            println!("    {:?}: {}", hs, count);
        }
    }

    let cooldown = match (status.scale_up_cooldown, status.scale_down_cooldown) {
        (true, true) => "up+down".red().to_string(),
        (true, false) => "up".yellow().to_string(),
        (false, true) => "down".yellow().to_string(),
        (false, false) => "none".green().to_string(),
    };
    println!("  Scale cooldown:    {}", cooldown);

    Ok(())
}

// ── Session ───────────────────────────────────────────────────────────

fn exec_session(runtime: &Runtime, cmd: &SessionCommands) -> anyhow::Result<()> {
    match cmd {
        SessionCommands::Create { id, tier } => {
            let billing_tier = parse_tier(tier)?;

            println!("{}", format!("Creating session '{}'...", id).green().bold());

            let info = runtime.create_session(id, billing_tier)?;

            println!("{}", "✓ Session created".green());
            print_session_info(&info);
        }
        SessionCommands::Destroy { id } => {
            println!(
                "{}",
                format!("Destroying session '{}'...", id).yellow().bold()
            );

            let invoice = runtime.destroy_session(id)?;

            println!("{}", format!("✓ Session '{}' destroyed", id).green());
            if let Some(inv) = invoice {
                println!(
                    "  Invoice: ${:.2} ({} line items)",
                    inv.total(),
                    inv.items.len()
                );
            } else {
                println!("  No invoice generated");
            }
        }
    }
    Ok(())
}

fn print_session_info(info: &SessionInfo) {
    println!("  Session ID:  {}", info.session_id.cyan());
    println!("  VM ID:       {}", info.vm_id.green());
    println!("  Tier:        {:?}", info.tier);
}

// ── Workload ──────────────────────────────────────────────────────────

fn exec_workload(runtime: &Runtime, cmd: &WorkloadCommands) -> anyhow::Result<()> {
    match cmd {
        WorkloadCommands::Submit {
            id,
            session,
            vcpus,
            memory,
            gpu,
            priority,
            timeout,
        } => {
            println!(
                "{}",
                format!("Submitting workload '{}'...", id).green().bold()
            );

            let descriptor = WorkloadDescriptor {
                id: id.clone(),
                session_id: session.clone(),
                required_vcpus: *vcpus,
                required_memory: *memory,
                requires_gpu: *gpu,
                priority: *priority,
                constraints: Vec::new(),
                submitted_at: SystemTime::now(),
                placement_timeout: Duration::from_secs(*timeout),
                attempts: 0,
            };

            let result = runtime.submit_workload(descriptor)?;
            print_workload_result(&result);
        }
        WorkloadCommands::Schedule => {
            println!("{}", "Scheduling pending workloads...".green().bold());

            let placements = runtime.schedule_pending()?;
            print_placements(&placements);
        }
    }
    Ok(())
}

fn print_workload_result(result: &WorkloadResult) {
    if result.placed {
        println!("{}", "✓ Workload placed".green());
        println!("  Workload ID: {}", result.workload_id.cyan());
        println!(
            "  VM ID:       {}",
            result.vm_id.as_deref().unwrap_or("none").green()
        );
    } else {
        println!("{}", "⏳ Workload queued (no placement yet)".yellow());
        println!("  Workload ID: {}", result.workload_id.cyan());
    }
}

fn print_placements(placements: &[Placement]) {
    if placements.is_empty() {
        println!("{}", "No workloads scheduled (queue empty)".yellow());
    } else {
        println!(
            "{}",
            format!("✓ Scheduled {} workload(s)", placements.len()).green()
        );
        for p in placements {
            println!(
                "  {} → {} (score: {:.2})",
                p.workload_id.cyan(),
                p.vm_id.green(),
                p.score
            );
        }
    }
}

// ── Workflow ──────────────────────────────────────────────────────────

fn exec_workflow(runtime: &Runtime, cmd: &WorkflowCommands) -> anyhow::Result<()> {
    match cmd {
        WorkflowCommands::Run { spec } => {
            println!("{}", "Running workflow...".green().bold());

            let spec_content = std::fs::read_to_string(spec)
                .map_err(|e| anyhow::anyhow!("Failed to read spec file '{}': {}", spec, e))?;

            let workflow_spec: WorkflowSpec = serde_json::from_str(&spec_content)
                .map_err(|e| anyhow::anyhow!("Invalid workflow spec JSON: {}", e))?;

            let workflow_id = runtime.run_workflow(workflow_spec)?;
            println!("{}", "✓ Workflow started".green());
            println!("  Workflow ID: {}", workflow_id.cyan());
        }
        WorkflowCommands::Advance {
            workflow,
            step,
            outcome,
        } => {
            println!(
                "{}",
                format!("Advancing step '{}' in workflow '{}'...", step, workflow)
                    .green()
                    .bold()
            );

            let step_outcome = parse_outcome(outcome)?;
            let ready = runtime.advance_workflow_step(workflow, step, step_outcome)?;

            println!("{}", "✓ Step advanced".green());
            if ready.is_empty() {
                println!("  No more ready steps (workflow may be complete)");
            } else {
                println!("  Ready steps: {}", ready.join(", ").cyan());
            }
        }
        WorkflowCommands::Cancel { id } => {
            println!(
                "{}",
                format!("Cancelling workflow '{}'...", id).yellow().bold()
            );
            runtime.cancel_workflow(id)?;
            println!("{}", format!("✓ Workflow '{}' cancelled", id).green());
        }
    }
    Ok(())
}

// ── Maintenance ───────────────────────────────────────────────────────

fn exec_maintenance(runtime: &Runtime) -> anyhow::Result<()> {
    println!("{}", "Running maintenance tick...".cyan().bold());

    let report = runtime.maintenance_tick();

    println!("{}", "✓ Maintenance complete".green());
    if !report.unhealthy_removed.is_empty() {
        println!(
            "  Unhealthy removed: {}",
            report.unhealthy_removed.join(", ").red()
        );
    }
    if !report.degraded_detected.is_empty() {
        println!(
            "  Degraded detected: {}",
            report.degraded_detected.join(", ").yellow()
        );
    }
    if !report.sessions_expired.is_empty() {
        println!(
            "  Sessions expired:  {}",
            report.sessions_expired.join(", ").yellow()
        );
    }
    if let Some(ref decision) = report.scale_decision {
        println!("  Scale decision:    {:?}", decision);
    }
    if report.vms_provisioned > 0 {
        println!(
            "  VMs provisioned:   {}",
            report.vms_provisioned.to_string().green()
        );
    }
    if report.vms_terminated > 0 {
        println!(
            "  VMs terminated:    {}",
            report.vms_terminated.to_string().red()
        );
    }
    if report.store_gc_count > 0 {
        println!(
            "  Store GC'd:        {}",
            report.store_gc_count.to_string().cyan()
        );
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Parse a billing tier string into a `BillingTier`.
fn parse_tier(tier: &str) -> anyhow::Result<BillingTier> {
    match tier.to_lowercase().as_str() {
        "free" => Ok(BillingTier::Free),
        "standard" => Ok(BillingTier::Standard),
        "premium" => Ok(BillingTier::Premium),
        "enterprise" => Ok(BillingTier::Enterprise),
        _ => Err(anyhow::anyhow!(
            "Invalid tier '{}'. Must be one of: free, standard, premium, enterprise",
            tier
        )),
    }
}

/// Parse a step outcome string.
///
/// Formats:
/// - `"success"` → `StepOutcome::Success { output: None }`
/// - `"success:<output>"` → `StepOutcome::Success { output: Some(output) }`
/// - `"failure:<message>"` → `StepOutcome::Failure { error, retryable: false }`
/// - `"skip:<reason>"` → `StepOutcome::Skipped { reason }`
fn parse_outcome(outcome: &str) -> anyhow::Result<StepOutcome> {
    if outcome == "success" {
        return Ok(StepOutcome::Success { output: None });
    }
    if let Some(msg) = outcome.strip_prefix("success:") {
        return Ok(StepOutcome::Success {
            output: Some(msg.to_string()),
        });
    }
    if let Some(msg) = outcome.strip_prefix("failure:") {
        return Ok(StepOutcome::Failure {
            error: msg.to_string(),
            retryable: false,
        });
    }
    if let Some(msg) = outcome.strip_prefix("skip:") {
        return Ok(StepOutcome::Skipped {
            reason: msg.to_string(),
        });
    }
    Err(anyhow::anyhow!(
        "Invalid outcome '{}'. Use: success, success:<output>, failure:<msg>, or skip:<reason>",
        outcome
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_runtime::{PoolConfig, RuntimeConfig};

    fn test_runtime(warm_vms: usize) -> Runtime {
        let mut config = RuntimeConfig::default();
        config.pool = PoolConfig {
            min_warm: warm_vms,
            max_size: warm_vms * 2,
            ..Default::default()
        };
        let rt = Runtime::new(config);

        // Pre-warm the pool
        for _ in 0..warm_vms {
            let vm_id = rt.pool().provision().unwrap();
            rt.pool().mark_warm(&vm_id).unwrap();
        }

        rt
    }

    // ── parse_tier ────────────────────────────────────────────────────

    #[test]
    fn test_parse_tier_free() {
        assert_eq!(parse_tier("free").unwrap(), BillingTier::Free);
    }

    #[test]
    fn test_parse_tier_standard() {
        assert_eq!(parse_tier("standard").unwrap(), BillingTier::Standard);
    }

    #[test]
    fn test_parse_tier_premium() {
        assert_eq!(parse_tier("Premium").unwrap(), BillingTier::Premium);
    }

    #[test]
    fn test_parse_tier_enterprise() {
        assert_eq!(parse_tier("ENTERPRISE").unwrap(), BillingTier::Enterprise);
    }

    #[test]
    fn test_parse_tier_invalid() {
        assert!(parse_tier("bronze").is_err());
    }

    // ── parse_outcome ─────────────────────────────────────────────────

    #[test]
    fn test_parse_outcome_success() {
        let o = parse_outcome("success").unwrap();
        assert!(o.is_success());
    }

    #[test]
    fn test_parse_outcome_success_with_output() {
        let o = parse_outcome("success:done").unwrap();
        match o {
            StepOutcome::Success { output } => assert_eq!(output, Some("done".to_string())),
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_parse_outcome_failure() {
        let o = parse_outcome("failure:oops").unwrap();
        match o {
            StepOutcome::Failure { error, retryable } => {
                assert_eq!(error, "oops");
                assert!(!retryable);
            }
            _ => panic!("Expected Failure"),
        }
    }

    #[test]
    fn test_parse_outcome_skip() {
        let o = parse_outcome("skip:not needed").unwrap();
        match o {
            StepOutcome::Skipped { reason } => assert_eq!(reason, "not needed"),
            _ => panic!("Expected Skipped"),
        }
    }

    #[test]
    fn test_parse_outcome_invalid() {
        assert!(parse_outcome("retry:later").is_err());
    }

    // ── exec_status ───────────────────────────────────────────────────

    #[test]
    fn test_exec_status() {
        let runtime = test_runtime(2);
        let result = exec_status(&runtime);
        assert!(result.is_ok());
    }

    // ── exec_session ──────────────────────────────────────────────────

    #[test]
    fn test_exec_session_create() {
        let runtime = test_runtime(2);
        let cmd = SessionCommands::Create {
            id: "test-sess".to_string(),
            tier: "standard".to_string(),
        };
        let result = exec_session(&runtime, &cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_session_create_no_vms() {
        let runtime = test_runtime(0);
        let cmd = SessionCommands::Create {
            id: "test-sess".to_string(),
            tier: "standard".to_string(),
        };
        let result = exec_session(&runtime, &cmd);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_session_destroy() {
        let runtime = test_runtime(2);
        // Create first
        runtime
            .create_session("destroy-me", BillingTier::Standard)
            .unwrap();

        let cmd = SessionCommands::Destroy {
            id: "destroy-me".to_string(),
        };
        let result = exec_session(&runtime, &cmd);
        assert!(result.is_ok());
    }

    // ── exec_workload ─────────────────────────────────────────────────

    #[test]
    fn test_exec_workload_submit() {
        let runtime = test_runtime(2);
        let cmd = WorkloadCommands::Submit {
            id: "wl-1".to_string(),
            session: "sess-1".to_string(),
            vcpus: 1,
            memory: 536870912,
            gpu: false,
            priority: 50,
            timeout: 30,
        };
        let result = exec_workload(&runtime, &cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_workload_schedule() {
        let runtime = test_runtime(2);
        let cmd = WorkloadCommands::Schedule;
        let result = exec_workload(&runtime, &cmd);
        assert!(result.is_ok());
    }

    // ── exec_workflow ─────────────────────────────────────────────────

    #[test]
    fn test_exec_workflow_cancel() {
        let runtime = test_runtime(2);

        // Create a workflow first
        let spec = WorkflowSpec {
            name: "cancel-me".to_string(),
            description: String::new(),
            steps: vec![hv2_runtime::StepSpec {
                name: "step-1".to_string(),
                description: String::new(),
                depends_on: vec![],
                timeout: Duration::from_secs(300),
                max_retries: 0,
                retry_delay: Duration::from_secs(5),
                command: "echo".to_string(),
                optional: false,
            }],
            timeout: Duration::from_secs(3600),
            variables: Default::default(),
            max_parallel_steps: 4,
        };
        let wf_id = runtime.run_workflow(spec).unwrap();

        let cmd = WorkflowCommands::Cancel { id: wf_id };
        let result = exec_workflow(&runtime, &cmd);
        assert!(result.is_ok());
    }

    // ── exec_maintenance ──────────────────────────────────────────────

    #[test]
    fn test_exec_maintenance() {
        let runtime = test_runtime(2);
        let result = exec_maintenance(&runtime);
        assert!(result.is_ok());
    }

    // ── execute (top-level dispatch) ──────────────────────────────────

    #[test]
    fn test_execute_status() {
        let runtime = test_runtime(2);
        let result = execute(&runtime, &RuntimeCommands::Status);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_maintenance() {
        let runtime = test_runtime(2);
        let result = execute(&runtime, &RuntimeCommands::Maintenance);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_session_lifecycle() {
        let runtime = test_runtime(4);

        // Create
        let result = execute(
            &runtime,
            &RuntimeCommands::Session(SessionCommands::Create {
                id: "lifecycle".to_string(),
                tier: "premium".to_string(),
            }),
        );
        assert!(result.is_ok());

        // Destroy
        let result = execute(
            &runtime,
            &RuntimeCommands::Session(SessionCommands::Destroy {
                id: "lifecycle".to_string(),
            }),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_workload_submit_and_schedule() {
        let runtime = test_runtime(2);

        // Submit
        let result = execute(
            &runtime,
            &RuntimeCommands::Workload(WorkloadCommands::Submit {
                id: "wl-exec".to_string(),
                session: "s1".to_string(),
                vcpus: 1,
                memory: 536870912,
                gpu: false,
                priority: 50,
                timeout: 30,
            }),
        );
        assert!(result.is_ok());

        // Schedule
        let result = execute(
            &runtime,
            &RuntimeCommands::Workload(WorkloadCommands::Schedule),
        );
        assert!(result.is_ok());
    }
}
