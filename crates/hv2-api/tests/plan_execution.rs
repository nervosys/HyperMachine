//! Ontology plans executed against a real host, and unwound when they fail.
//!
//! `/agentic/plans/execute` used to fabricate every result. These tests pin the
//! two things that changed: a plan's steps now reach a `VmHost`, and a failed
//! plan's rollback actually destroys what the plan created rather than merely
//! reporting that it did.

use std::sync::Arc;

use async_trait::async_trait;
use hv2_agent::{LocalVmHost, VmDescriptor, VmHost, VmSpec};
use hv2_api::ontology::{
    ActionPlan, HyperMachineOntology, PlanExecutionRequest, PlanExecutionStatus, PlanStep,
    VmHostExecutor,
};
use parking_lot::Mutex;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

fn step(step_id: &str, operation_id: &str, params: serde_json::Value) -> PlanStep {
    PlanStep {
        step_id: step_id.to_string(),
        operation_id: operation_id.to_string(),
        parameters: params,
        depends_on: Vec::new(),
        timeout_seconds: None,
    }
}

fn plan(name: &str, steps: Vec<PlanStep>, rollback: bool) -> PlanExecutionRequest {
    PlanExecutionRequest {
        plan: ActionPlan {
            name: name.to_string(),
            description: "test plan".to_string(),
            steps,
            rollback_on_failure: rollback,
        },
        dry_run: None,
        timeout_seconds: None,
        variables: None,
    }
}

/// A host that can be told to fail a specific operation, so a plan's failure
/// path can be exercised deterministically.
struct FlakyHost {
    inner: LocalVmHost,
    fail_start: bool,
    deleted: Mutex<Vec<String>>,
}

impl FlakyHost {
    fn failing_start() -> Self {
        Self {
            inner: LocalVmHost::new(),
            fail_start: true,
            deleted: Mutex::new(Vec::new()),
        }
    }

    fn deleted(&self) -> Vec<String> {
        self.deleted.lock().clone()
    }
}

#[async_trait]
impl VmHost for FlakyHost {
    async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String> {
        self.inner.create(spec).await
    }

    async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        if self.fail_start {
            return Err("no hypervisor backend available".to_string());
        }
        self.inner.start(vm_id).await
    }

    async fn stop(&self, vm_id: &str, force: bool) -> Result<VmDescriptor, String> {
        self.inner.stop(vm_id, force).await
    }

    async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.inner.pause(vm_id).await
    }

    async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.inner.resume(vm_id).await
    }

    async fn delete(&self, vm_id: &str) -> Result<(), String> {
        self.deleted.lock().push(vm_id.to_string());
        self.inner.delete(vm_id).await
    }

    async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.inner.status(vm_id).await
    }

    async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
        self.inner.list().await
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Real execution
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_plan_step_creates_a_real_vm() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "provision",
        vec![step(
            "s1",
            "create_vm",
            json!({"name": "planned-vm", "vcpu_count": 4, "memory_gb": 8}),
        )],
        false,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    assert_eq!(
        host.vm_count(),
        1,
        "the plan should have created an actual VM, not a JSON record"
    );

    let output = result.step_results[0].output.as_ref().unwrap();
    assert_eq!(output["name"], "planned-vm");
    assert_eq!(
        output["cpu_cores"], 4,
        "the spec must reach the host intact"
    );
    assert_eq!(output["memory_gb"], 8);
    assert!(
        output.get("simulated").is_none(),
        "a real result must not be tagged as simulated"
    );
}

#[tokio::test]
async fn simulated_execution_is_still_marked_as_simulated() {
    // The distinction has to survive: a caller must always be able to tell a
    // rehearsal from the real thing.
    let ontology = HyperMachineOntology::build();
    let request = plan("rehearsal", vec![step("s1", "list_vms", json!({}))], false);

    let result = ontology.execute_plan(&request);

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    assert_eq!(
        result.step_results[0].output.as_ref().unwrap()["simulated"],
        true
    );
}

#[tokio::test]
async fn an_operation_the_host_cannot_perform_is_refused_not_faked() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host);
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "script",
        vec![step(
            "s1",
            "execute_script",
            json!({"id": "vm-1", "script": "echo hi"}),
        )],
        false,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(
        result.status,
        PlanExecutionStatus::Failed,
        "a step with no real implementation must fail rather than fabricate output"
    );
    let error = result.step_results[0].error.as_ref().unwrap();
    assert!(error.contains("no VM-host implementation"), "got: {error}");
}

#[tokio::test]
async fn a_metrics_step_reports_the_hosts_real_numbers() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let created = host
        .create({
            let mut spec = VmSpec::new("measured");
            spec.cpu_cores = 6;
            spec.memory_gb = 12;
            spec
        })
        .await
        .unwrap();

    let request = plan(
        "observe",
        vec![step("s1", "get_metrics", json!({ "id": created.vm_id }))],
        false,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    let output = result.step_results[0].output.as_ref().unwrap();
    assert_eq!(output["vcpu_count"], 6);
    assert_eq!(output["memory_total_bytes"], 12u64 * 1024 * 1024 * 1024);
    assert!(
        output.get("simulated").is_none(),
        "telemetry from a real host must not be tagged as simulated"
    );
    // The fields the host cannot measure come back null. An agent must be able
    // to distinguish "not instrumented" from "idle".
    assert!(output["cpu_usage_percent"].is_null());
    assert!(output["memory_used_bytes"].is_null());
}

// ═══════════════════════════════════════════════════════════════════
//  Rollback
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_failed_plan_destroys_the_vm_it_created() {
    let host = Arc::new(FlakyHost::failing_start());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "provision-and-boot",
        vec![
            step("create", "create_vm", json!({"name": "doomed"})),
            // The plan references the created VM by a name the caller knows;
            // the start fails, which is what triggers the unwind.
            step("boot", "start_vm", json!({"id": "unused"})),
        ],
        true,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::RolledBack);
    assert!(
        result.rolled_back_steps.contains(&"create".to_string()),
        "the create step should have been compensated"
    );
    assert_eq!(
        host.deleted().len(),
        1,
        "rollback must actually delete the VM, not just claim to"
    );
    assert_eq!(
        host.inner.vm_count(),
        0,
        "no VM should be left behind by a rolled-back plan"
    );
}

#[tokio::test]
async fn rollback_uses_the_id_the_host_assigned() {
    // The plan never names the created VM's id — only the host knows it — so a
    // rollback that could not read it back from the step output would leak.
    let host = Arc::new(FlakyHost::failing_start());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "leak-check",
        vec![
            step("create", "create_vm", json!({"name": "tracked"})),
            step("boot", "start_vm", json!({"id": "whatever"})),
        ],
        true,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;
    let created_id = result.step_results[0].output.as_ref().unwrap()["vm_id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(host.deleted(), vec![created_id]);
}

#[tokio::test]
async fn a_plan_without_rollback_leaves_its_work_in_place() {
    let host = Arc::new(FlakyHost::failing_start());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "no-rollback",
        vec![
            step("create", "create_vm", json!({"name": "kept"})),
            step("boot", "start_vm", json!({"id": "x"})),
        ],
        false,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Failed);
    assert!(result.rolled_back_steps.is_empty());
    assert_eq!(
        host.inner.vm_count(),
        1,
        "without rollback_on_failure the created VM must survive"
    );
}

#[tokio::test]
async fn a_rollback_that_cannot_complete_is_reported_as_failed() {
    // Deleting a VM is irreversible. A plan that deletes and then fails must
    // not claim a clean unwind — the operator needs to know what is gone.
    let host = Arc::new(LocalVmHost::new());
    let created = host.create(VmSpec::new("victim")).await.unwrap();
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let request = plan(
        "destructive",
        vec![
            step("delete", "delete_vm", json!({"id": created.vm_id})),
            // Deleting the same VM again fails, triggering rollback.
            step("delete-again", "delete_vm", json!({"id": created.vm_id})),
        ],
        true,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(
        result.status,
        PlanExecutionStatus::Failed,
        "an incomplete rollback is not a RolledBack outcome"
    );
    assert!(
        !result.step_results[0].rolled_back,
        "a step that could not be undone must not be marked rolled back"
    );
    let error = result.step_results[0].error.as_ref().unwrap();
    assert!(error.contains("cannot be restored"), "got: {error}");
}

// ═══════════════════════════════════════════════════════════════════
//  Shared orchestration
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_dry_run_touches_nothing_even_with_a_real_executor() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let mut request = plan(
        "dry",
        vec![step("s1", "create_vm", json!({"name": "never-made"}))],
        false,
    );
    request.dry_run = Some(true);

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    assert!(result.step_results.is_empty());
    assert_eq!(host.vm_count(), 0, "a dry run must not create anything");
}

#[tokio::test]
async fn real_execution_honours_dependency_order() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    // Declared out of order; `create` must still run first or the list step
    // would see nothing.
    let mut list = step("list", "list_vms", json!({}));
    list.depends_on = vec!["create".to_string()];

    let request = plan(
        "ordered",
        vec![list, step("create", "create_vm", json!({"name": "first"}))],
        false,
    );

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    assert_eq!(result.step_results[0].step_id, "create");
    assert_eq!(result.step_results[1].step_id, "list");
    assert_eq!(
        result.step_results[1].output.as_ref().unwrap()["total"],
        1,
        "the list step should observe the VM the create step made"
    );
}

#[tokio::test]
async fn independent_steps_run_in_declared_order_every_time() {
    // Steps with no dependencies are topologically interchangeable, but a plan
    // has to execute the same way on every run — otherwise a plan that happens
    // to work is a coin flip, and a rollback unwinds a different sequence than
    // the one that ran.
    let ontology = HyperMachineOntology::build();
    let request = plan(
        "independent",
        vec![
            step("alpha", "list_vms", json!({})),
            step("bravo", "list_vms", json!({})),
            step("charlie", "list_vms", json!({})),
            step("delta", "list_vms", json!({})),
            step("echo", "list_vms", json!({})),
        ],
        false,
    );

    let expected = vec!["alpha", "bravo", "charlie", "delta", "echo"];

    // Repeat: a hash-ordered implementation passes a single run by luck.
    for attempt in 0..25 {
        let result = ontology.execute_plan(&request);
        let order: Vec<&str> = result
            .step_results
            .iter()
            .map(|r| r.step_id.as_str())
            .collect();
        assert_eq!(order, expected, "run {attempt} executed out of order");
    }
}

// ═══════════════════════════════════════════════════════════════════
//  The server's own wiring
// ═══════════════════════════════════════════════════════════════════

/// A plan executed through the HTTP endpoint must act on the same VMs the
/// `/api/v1/vms` endpoints manage — a plan that created VMs the REST API
/// could not see would be worse than no plan execution at all.
#[tokio::test]
async fn a_plan_executed_over_http_creates_a_vm_the_rest_api_can_see() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = hv2_api::rest::create_router();

    let plan_body = json!({
        "plan": {
            "name": "http-provision",
            "description": "create a VM through the plan endpoint",
            "steps": [{
                "step_id": "create",
                "operation_id": "create_vm",
                "parameters": {"name": "via-plan", "vcpu_count": 2, "memory_gb": 1},
                "depends_on": [],
                "timeout_seconds": null
            }],
            "rollback_on_failure": false
        }
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/agentic/plans/execute")
                .header("content-type", "application/json")
                .body(Body::from(plan_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["status"], "completed", "plan result: {result}");
    assert!(
        result["step_results"][0]["output"]
            .get("simulated")
            .is_none(),
        "the served endpoint must execute for real, not simulate"
    );

    // The REST API must now list the VM the plan created.
    let listed = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/vms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(listed.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        body.contains("via-plan"),
        "the plan and the REST endpoints must share one VM inventory; got: {body}"
    );
}

#[tokio::test]
async fn real_execution_substitutes_variables() {
    let host = Arc::new(LocalVmHost::new());
    let executor = VmHostExecutor::new(host.clone());
    let ontology = HyperMachineOntology::build();

    let mut request = plan(
        "parameterized",
        vec![step("s1", "create_vm", json!({"name": "${vm_name}"}))],
        false,
    );
    request.variables = Some(serde_json::from_value(json!({"vm_name": "substituted"})).unwrap());

    let result = ontology.execute_plan_with(&request, &executor).await;

    assert_eq!(result.status, PlanExecutionStatus::Completed);
    assert_eq!(
        result.step_results[0].output.as_ref().unwrap()["name"],
        "substituted"
    );
}
