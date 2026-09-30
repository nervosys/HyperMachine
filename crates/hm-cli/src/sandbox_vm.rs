//! Client for the VM sandbox API. Host confinement remains `hm sandbox run`.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use reqwest::{Client, Method, Url};
use serde_json::{json, Value};

/// Connection and operation for a remote VM sandbox.
#[derive(Debug, Args)]
pub struct VmArgs {
    /// Sandbox API URL; defaults to HV2_SANDBOX_URL or http://127.0.0.1:8080
    #[arg(long, global = true)]
    pub endpoint: Option<String>,
    /// HTTP deadline in seconds, including guest command execution
    #[arg(long, global = true, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..))]
    pub request_timeout: u64,
    #[command(subcommand)]
    pub command: VmCommand,
}

/// VM sandbox lifecycle commands. Responses are JSON for shell automation.
#[derive(Debug, Subcommand)]
pub enum VmCommand {
    /// Measure creation through a verified guest command, with bounded concurrency
    Benchmark {
        #[arg(long, default_value = "base")]
        template: String,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=10000))]
        samples: u32,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=1000))]
        concurrency: u32,
        /// Description of the hardware, virtualization and template configuration
        #[arg(long)]
        environment: String,
        /// Fail if successful samples' P99 ready time exceeds this deadline
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        max_p99_ready_ms: Option<u64>,
    },
    /// Create a VM from a prepared template
    Create {
        #[arg(long, default_value = "base")]
        template: String,
        /// Sandbox lifetime in seconds
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
        lifetime: u64,
    },
    /// List VM sandboxes
    List,
    /// Inspect a VM sandbox
    Inspect { id: String },
    /// Run a program inside a VM; prints its streams and exits with its code
    Exec {
        id: String,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Pause a VM to disk
    Pause { id: String },
    /// Resume a paused VM with a new lifetime
    Resume {
        id: String,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
        lifetime: u64,
    },
    /// Fork a running VM, including memory
    Fork {
        id: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=100))]
        count: u32,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
        lifetime: u64,
    },
    /// Destroy a VM sandbox
    Delete { id: String },
    /// Save, list, restore, or delete a checkpoint belonging to a VM
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
}

/// In-place checkpoints preserve the VM's identity on restore.
#[derive(Debug, Subcommand)]
pub enum CheckpointCommand {
    Save { id: String, name: String },
    List { id: String },
    Restore { id: String, name: String },
    Delete { id: String, name: String },
}

#[derive(Clone)]
struct Api {
    client: Client,
    base: Url,
}

impl Api {
    fn new(endpoint: &str, timeout: u64, key: Option<String>) -> Result<Self> {
        let base = Url::parse(endpoint).context("invalid sandbox endpoint")?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            bail!("sandbox endpoint must be an HTTP(S) URL without credentials, query or fragment");
        }
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            let mut value = reqwest::header::HeaderValue::from_str(&key)
                .map_err(|_| anyhow::anyhow!("HV2_API_KEY is not a valid HTTP header value"))?;
            value.set_sensitive(true);
            headers.insert("x-api-key", value);
        }
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(timeout))
            .build()?;
        Ok(Self { client, base })
    }

    async fn request(&self, method: Method, path: &[&str], body: Option<Value>) -> Result<Value> {
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid endpoint"))?;
            segments.pop_if_empty();
            for segment in path {
                // Dot segments are special to URLs; don't silently change the route.
                if segment.is_empty() || matches!(*segment, "." | "..") {
                    bail!("sandbox IDs and checkpoint names must not be empty or dot segments");
                }
                segments.push(segment);
            }
        }
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("sandbox API request failed")?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("sandbox API returned {status}: {body}");
        }
        if body.trim().is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_str(&body).context("sandbox API returned invalid JSON")
        }
    }
}

/// Execute the operation. API errors fail; guest errors preserve their exit code.
pub async fn run(args: VmArgs) -> Result<i32> {
    let endpoint = args
        .endpoint
        .or_else(|| std::env::var("HV2_SANDBOX_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let api = Api::new(
        &endpoint,
        args.request_timeout,
        std::env::var("HV2_API_KEY").ok(),
    )?;
    let value = match args.command {
        VmCommand::Benchmark {
            template,
            samples,
            concurrency,
            environment,
            max_p99_ready_ms,
        } => {
            return benchmark(
                api,
                template,
                samples,
                concurrency,
                environment,
                max_p99_ready_ms,
            )
            .await;
        }
        VmCommand::Create { template, lifetime } => {
            api.request(
                Method::POST,
                &["v2", "sandboxes"],
                Some(json!({"templateID": template, "timeout": lifetime})),
            )
            .await?
        }
        VmCommand::List => api.request(Method::GET, &["sandboxes"], None).await?,
        VmCommand::Inspect { id } => api.request(Method::GET, &["sandboxes", &id], None).await?,
        VmCommand::Delete { id } => {
            api.request(Method::DELETE, &["sandboxes", &id], None)
                .await?
        }
        VmCommand::Pause { id } => {
            api.request(Method::POST, &["sandboxes", &id, "pause"], None)
                .await?
        }
        VmCommand::Resume { id, lifetime } => {
            api.request(
                Method::POST,
                &["sandboxes", &id, "resume"],
                Some(json!({"timeout": lifetime})),
            )
            .await?
        }
        VmCommand::Fork {
            id,
            count,
            lifetime,
        } => {
            api.request(
                Method::POST,
                &["sandboxes", &id, "fork"],
                Some(json!({"count": count, "timeout": lifetime})),
            )
            .await?
        }
        VmCommand::Exec {
            id,
            timeout,
            command,
        } => {
            if timeout >= args.request_timeout {
                bail!("--request-timeout must exceed the guest command's --timeout");
            }
            let cmd = shell_exec(&command)?;
            let value = api
                .request(
                    Method::POST,
                    &["sandboxes", &id, "exec"],
                    Some(json!({"cmd": cmd, "timeout_secs": timeout})),
                )
                .await?;
            let stdout = value["stdout"]
                .as_str()
                .context("exec response missing stdout")?;
            let stderr = value["stderr"]
                .as_str()
                .context("exec response missing stderr")?;
            let timed_out = value["timed_out"]
                .as_bool()
                .context("exec response missing timed_out")?;
            let code = exec_exit_code(&value, timed_out)?;
            use std::io::Write;
            std::io::stdout().write_all(stdout.as_bytes())?;
            std::io::stderr().write_all(stderr.as_bytes())?;
            return Ok(code);
        }
        VmCommand::Checkpoint { command } => match command {
            CheckpointCommand::Save { id, name } => {
                api.request(
                    Method::POST,
                    &["sandboxes", &id, "checkpoints"],
                    Some(json!({"name": name})),
                )
                .await?
            }
            CheckpointCommand::List { id } => {
                api.request(Method::GET, &["sandboxes", &id, "checkpoints"], None)
                    .await?
            }
            CheckpointCommand::Restore { id, name } => {
                api.request(
                    Method::POST,
                    &["sandboxes", &id, "checkpoints", &name, "restore"],
                    None,
                )
                .await?
            }
            CheckpointCommand::Delete { id, name } => {
                api.request(
                    Method::DELETE,
                    &["sandboxes", &id, "checkpoints", &name],
                    None,
                )
                .await?
            }
        },
    };
    if !value.is_null() {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(0)
}

fn exec_exit_code(value: &Value, timed_out: bool) -> Result<i32> {
    if timed_out {
        return Ok(124);
    }
    match value["exit_code"].as_i64() {
        Some(code @ 0..=255) => Ok(code as i32),
        None if value["exit_code"].is_null() => Ok(1),
        _ => bail!("exec response has an invalid exit code"),
    }
}

// sandboxd's /exec always invokes /bin/sh -c cmd. Quote argv here so callers
// cannot accidentally turn a literal argument into shell syntax.
fn shell_exec(command: &[String]) -> Result<String> {
    if command.is_empty() {
        bail!("a guest command is required");
    }
    if command.iter().any(|arg| arg.contains('\0')) {
        bail!("guest arguments cannot contain NUL");
    }
    Ok(format!(
        "exec {}",
        command
            .iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ")
    ))
}

/// One sample owns and destroys its sandbox even when its readiness check fails.
async fn benchmark_sample(api: Api, template: String, index: u32) -> Value {
    let started = std::time::Instant::now();
    let created = match api
        .request(
            Method::POST,
            &["v2", "sandboxes"],
            Some(json!({
                "templateID": template, "timeout": 300,
            })),
        )
        .await
    {
        Ok(created) => created,
        Err(error) => {
            return json!({"index": index, "success": false, "phase": "create", "error": error.to_string()})
        }
    };
    let create_ms = started.elapsed().as_secs_f64() * 1000.0;
    let Some(id) = created["sandboxID"].as_str().filter(|id| !id.is_empty()) else {
        return json!({"index": index, "success": false, "phase": "create", "error": "response missing sandboxID"});
    };
    // A successful API response is not proof the guest can execute. Assert the effect.
    let marker = format!("hm-benchmark-ready-{index}");
    let exec_started = std::time::Instant::now();
    let ready = api
        .request(
            Method::POST,
            &["sandboxes", id, "exec"],
            Some(json!({
                    "cmd": format!("printf '%s' '{marker}'"), "timeout_secs": 30,
            })),
        )
        .await
        .and_then(|value| {
            if value["exit_code"] != 0 || value["timed_out"] != false || value["stdout"] != marker {
                bail!("guest readiness command did not produce the expected output");
            }
            Ok(())
        });
    let exec_ms = exec_started.elapsed().as_secs_f64() * 1000.0;
    let ready_ms = started.elapsed().as_secs_f64() * 1000.0;
    let cleanup = api.request(Method::DELETE, &["sandboxes", id], None).await;
    match (ready, cleanup) {
        (Ok(()), Ok(_)) => {
            json!({"index": index, "success": true, "create_ms": create_ms, "exec_ms": exec_ms, "ready_ms": ready_ms})
        }
        (ready, cleanup) => {
            json!({"index": index, "success": false, "phase": "ready_or_cleanup", "create_ms": create_ms,
            "ready_error": ready.err().map(|e| e.to_string()), "cleanup_error": cleanup.err().map(|e| e.to_string())})
        }
    }
}

fn latency_summary(mut values: Vec<f64>) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    // Nearest-rank percentiles: a small sample set cannot invent extra tail data.
    let percentile = |q: f64| values[((q * values.len() as f64).ceil() as usize).saturating_sub(1)];
    json!({"n": values.len(), "min": values[0], "p50": percentile(0.50), "p95": percentile(0.95),
        "p99": percentile(0.99), "max": values[values.len()-1], "mean": values.iter().sum::<f64>() / values.len() as f64})
}

async fn benchmark(
    api: Api,
    template: String,
    samples: u32,
    concurrency: u32,
    environment: String,
    max_p99_ready_ms: Option<u64>,
) -> Result<i32> {
    if environment.trim().is_empty() {
        bail!("--environment must describe the test environment");
    }
    let mut pending = tokio::task::JoinSet::new();
    let mut records = Vec::new();
    let started = std::time::Instant::now();
    for index in 0..samples {
        if pending.len() >= concurrency as usize {
            records.push(
                pending
                    .join_next()
                    .await
                    .context("benchmark task missing")??,
            );
        }
        pending.spawn(benchmark_sample(api.clone(), template.clone(), index));
    }
    while let Some(record) = pending.join_next().await {
        records.push(record?);
    }
    records.sort_by_key(|r| r["index"].as_u64());
    let elapsed = started.elapsed().as_secs_f64();
    let success = records.iter().filter(|r| r["success"] == true).count();
    let summarize = |name: &str| {
        latency_summary(
            records
                .iter()
                .filter(|r| r["success"] == true)
                .filter_map(|r| r[name].as_f64())
                .collect(),
        )
    };
    let ready = summarize("ready_ms");
    let threshold_passed =
        max_p99_ready_ms.map(|max| ready["p99"].as_f64().is_some_and(|p99| p99 <= max as f64));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1, "client_version": env!("CARGO_PKG_VERSION"), "environment": environment,
            "template": template, "requested_samples": samples, "concurrency": concurrency,
            "successful_samples": success, "failed_samples": samples as usize - success,
            "elapsed_seconds": elapsed, "completed_lifecycles_per_second": success as f64 / elapsed,
            "readiness": "creation followed by /bin/sh producing a checked unique marker",
            "create_ms": summarize("create_ms"), "exec_ms": summarize("exec_ms"), "ready_ms": ready,
            "max_p99_ready_ms": max_p99_ready_ms, "threshold_passed": threshold_passed,
            "samples": records,
        }))?
    );
    Ok(
        if success == samples as usize && threshold_passed != Some(false) {
            0
        } else {
            1
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_arguments_keep_quotes_empty_values_and_shell_syntax_literal() {
        let args = ["printf", "%s", "a'b", "", "$(touch /tmp/escape); `id`"].map(String::from);
        assert_eq!(
            shell_exec(&args).unwrap(),
            "exec 'printf' '%s' 'a'\\''b' '' '$(touch /tmp/escape); `id`'"
        );
        assert!(shell_exec(&["bad\0argument".into()]).is_err());
    }

    #[test]
    fn percentiles_keep_the_tail_and_empty_samples_are_not_zero() {
        assert!(latency_summary(vec![]).is_null());
        let summary = latency_summary(vec![1.0, 2.0, 100.0]);
        assert_eq!(summary["p50"], 2.0);
        assert_eq!(summary["p99"], 100.0);
    }

    #[tokio::test]
    async fn failed_readiness_still_deletes_the_sandbox() {
        use axum::{
            extract::State,
            routing::{delete, post},
            Router,
        };
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let deleted = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route(
                "/v2/sandboxes",
                post(|| async { axum::Json(json!({"sandboxID": "test-vm"})) }),
            )
            .route(
                "/sandboxes/test-vm/exec",
                post(|| async {
                    axum::Json(
                        json!({"exit_code": 0, "timed_out": false, "stdout": "wrong marker"}),
                    )
                }),
            )
            .route(
                "/sandboxes/test-vm",
                delete(|State(deleted): State<Arc<AtomicBool>>| async move {
                    deleted.store(true, Ordering::SeqCst);
                    axum::http::StatusCode::NO_CONTENT
                }),
            )
            .with_state(deleted.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let api = Api::new(&format!("http://{addr}"), 5, None).unwrap();
        let record = benchmark_sample(api, "base".into(), 0).await;
        assert_eq!(record["success"], false);
        assert!(deleted.load(Ordering::SeqCst));
        server.abort();
    }

    #[test]
    fn guest_failure_and_timeout_are_not_success() {
        assert_eq!(exec_exit_code(&json!({"exit_code": 7}), false).unwrap(), 7);
        assert_eq!(
            exec_exit_code(&json!({"exit_code": null}), false).unwrap(),
            1
        );
        assert_eq!(exec_exit_code(&json!({"exit_code": 0}), true).unwrap(), 124);
        assert!(exec_exit_code(&json!({"exit_code": 999}), false).is_err());
    }

    #[tokio::test]
    async fn sends_api_key_and_body_and_reports_server_errors() {
        use axum::{extract::Json, http::HeaderMap, routing::post, Router};
        let app = Router::new().route(
            "/v2/sandboxes",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["x-api-key"], "test-key");
                assert_eq!(body["templateID"], "python");
                (axum::http::StatusCode::CONFLICT, "template is not ready")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let api = Api::new(&format!("http://{addr}"), 5, Some("test-key".into())).unwrap();
        let error = api
            .request(
                Method::POST,
                &["v2", "sandboxes"],
                Some(json!({"templateID": "python"})),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("409"));
        assert!(error.contains("template is not ready"));
        server.abort();
    }
}
