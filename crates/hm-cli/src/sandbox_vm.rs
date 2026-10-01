//! Client for the VM sandbox API. Host confinement remains `hm sandbox run`.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use reqwest::{Client, Method, Url};
use serde_json::{json, Value};

#[path = "sandbox_vm_mcp.rs"]
mod mcp;

/// Connection and operation for a remote VM sandbox.
#[derive(Debug, Args)]
pub struct VmArgs {
    /// Sandbox API URL; defaults to HV2_SANDBOX_URL or http://127.0.0.1:3980
    #[arg(long, global = true)]
    pub endpoint: Option<String>,
    /// Additional PEM root certificate for the sandbox API
    #[arg(long, global = true)]
    pub api_ca_cert: Option<std::path::PathBuf>,
    /// HTTP deadline in seconds, including guest command execution
    #[arg(long, global = true, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..))]
    pub request_timeout: u64,
    #[command(subcommand)]
    pub command: VmCommand,
}

/// VM sandbox lifecycle commands. Responses are JSON for shell automation.
#[derive(Debug, Subcommand)]
pub enum VmCommand {
    /// Serve remote sandbox lifecycle tools using the MCP stdio protocol
    Mcp {
        /// Operator-selected envd endpoint for binary file tools
        #[arg(long)]
        envd_endpoint: Option<String>,
        /// Proxy domain appended to the sandbox's envd host label
        #[arg(long, requires = "envd_endpoint")]
        envd_domain: Option<String>,
    },
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
        /// Refuse a run unless the server confirms a prepared snapshot template
        #[arg(long)]
        require_snapshot: bool,
    },
    /// Create a VM from a prepared template
    Create {
        /// Persist a human-readable label for named stdio connections
        #[arg(long, value_parser = sandbox_name)]
        name: Option<String>,
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
    /// Connect stdin/stdout to a guest TCP port (for OpenSSH ProxyCommand)
    TcpStdio {
        #[arg(required_unless_present = "name", conflicts_with = "name")]
        id: Option<String>,
        /// Resolve an exact hm.name metadata label; duplicate names fail
        #[arg(long, conflicts_with = "id", value_parser = sandbox_name)]
        name: Option<String>,
        #[arg(long, default_value_t = 22, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
    },
    /// Forward a loopback TCP listener to a running VM's guest port
    Tcp {
        id: String,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Local listening address; only loopback addresses are accepted
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: std::net::SocketAddr,
        #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u32).range(1..=1024))]
        max_connections: u32,
    },
    /// Transfer binary files through the sandbox's authenticated envd endpoint
    Files {
        id: String,
        /// Reachable envd listener or sandbox proxy URL; use TLS remotely
        #[arg(long)]
        envd_endpoint: String,
        /// Virtual host for a sandbox proxy reached by IP or a shared hostname
        #[arg(long)]
        envd_host: Option<String>,
        #[command(subcommand)]
        command: VmFileCommand,
    },
    /// Save, list, restore, or delete a checkpoint belonging to a VM
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
    /// Bind operator-managed DNS hostnames to a VM's guest port (cluster API)
    Domain {
        #[command(subcommand)]
        command: DomainCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DomainCommand {
    /// Claim a hostname or change its port; configure DNS and TLS separately
    Bind {
        id: String,
        domain: String,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
    },
    /// List a VM's hostname bindings
    List { id: String },
    /// Release a hostname owned by this VM
    Unbind { id: String, domain: String },
}

/// File bytes travel through envd, without shell interpretation.
#[derive(Debug, Subcommand)]
pub enum VmFileCommand {
    Upload {
        source: std::path::PathBuf,
        path: String,
    },
    /// Download atomically; refuses to overwrite an existing destination
    Download {
        path: String,
        destination: std::path::PathBuf,
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
pub(crate) struct Api {
    client: Client,
    tcp_client: Client,
    base: Url,
}

impl Api {
    fn new(endpoint: &str, timeout: u64, key: Option<String>) -> Result<Self> {
        Self::with_ca(endpoint, timeout, key, None)
    }

    pub(crate) fn with_ca(
        endpoint: &str,
        timeout: u64,
        key: Option<String>,
        ca: Option<&std::path::Path>,
    ) -> Result<Self> {
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
        let certificate = ca
            .map(|path| -> Result<_> {
                let bytes = std::fs::read(path).context("could not read API CA certificate")?;
                reqwest::Certificate::from_pem(&bytes).context("invalid API CA certificate")
            })
            .transpose()?;
        let builder = || {
            let mut builder = Client::builder()
                .default_headers(headers.clone())
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(timeout));
            if let Some(certificate) = &certificate {
                builder = builder.add_root_certificate(certificate.clone());
            }
            builder
        };
        let client = builder().build()?;
        let tcp_client = builder().http1_only().build()?;
        Ok(Self {
            client,
            tcp_client,
            base,
        })
    }

    fn url(&self, path: &[&str]) -> Result<Url> {
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
        Ok(url)
    }

    async fn tcp(&self, id: &str, port: u16) -> Result<reqwest::Upgraded> {
        let port = port.to_string();
        let response = self
            .tcp_client
            .get(self.url(&["sandboxes", id, "ports", &port, "tcp"])?)
            .version(reqwest::Version::HTTP_11)
            .header("connection", "upgrade")
            .header("upgrade", "hv2-tcp/1")
            .send()
            .await
            .context("TCP tunnel handshake failed")?;
        if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            bail!("TCP tunnel API returned {}", response.status());
        }
        if response
            .headers()
            .get("upgrade")
            .and_then(|v| v.to_str().ok())
            != Some("hv2-tcp/1")
        {
            bail!("TCP tunnel API selected an unsupported protocol");
        }
        response
            .upgrade()
            .await
            .context("TCP tunnel upgrade failed")
    }

    async fn resolve_name(&self, name: &str) -> Result<String> {
        sandbox_name(name).map_err(anyhow::Error::msg)?;
        // v1 listing includes all running and paused matches, without v2's
        // page limit. Refuse ambiguity instead of choosing a first page/row.
        let response = self
            .client
            .get(self.url(&["sandboxes"])?)
            .query(&[("metadata", format!("hm.name={name}"))])
            .send()
            .await
            .context("sandbox name lookup failed")?;
        if !response.status().is_success() {
            bail!("sandbox name lookup returned {}", response.status());
        }
        let records: Value = response
            .json()
            .await
            .context("invalid sandbox name lookup response")?;
        let records = records
            .as_array()
            .context("sandbox name lookup must return an array")?;
        let matches: Vec<_> = records
            .iter()
            .filter(|row| row["metadata"]["hm.name"].as_str() == Some(name))
            .collect();
        match matches.as_slice() {
            [] => bail!("no sandbox named {name:?}"),
            [record] => {
                let id = record["sandboxID"]
                    .as_str()
                    .context("named sandbox missing sandboxID")?;
                // Validate path handling before connecting to the selected ID.
                self.url(&["sandboxes", id])?;
                Ok(id.to_owned())
            }
            _ => bail!("sandbox name {name:?} is ambiguous; use a sandbox ID"),
        }
    }

    pub(crate) async fn request_bounded(
        &self,
        method: Method,
        path: &[&str],
        body: Option<Value>,
        limit: usize,
    ) -> Result<Value> {
        let mut request = self.client.request(method, self.url(path)?);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.context("sandbox API request failed")?;
        if !response.status().is_success() {
            bail!("sandbox API returned {}", response.status());
        }
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            bail!("sandbox response exceeds byte limit");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                bail!("sandbox response exceeds byte limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
            Ok(Value::Null)
        } else {
            serde_json::from_slice(&bytes).context("sandbox API returned invalid JSON")
        }
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &[&str],
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self.client.request(method, self.url(path)?);
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
        .unwrap_or_else(|| "http://127.0.0.1:3980".into());
    let api = Api::with_ca(
        &endpoint,
        args.request_timeout,
        std::env::var("HV2_API_KEY").ok(),
        args.api_ca_cert.as_deref(),
    )?;
    let value = match args.command {
        VmCommand::TcpStdio { id, name, port } => {
            let id = match id {
                Some(id) => id,
                None => {
                    api.resolve_name(name.as_deref().context("sandbox name required")?)
                        .await?
                }
            };
            tcp_stdio(api.tcp(&id, port).await?).await?;
            return Ok(0);
        }
        VmCommand::Tcp {
            id,
            port,
            listen,
            max_connections,
        } => {
            tcp_forward(api, id, port, listen, max_connections).await?;
            return Ok(0);
        }
        VmCommand::Mcp {
            envd_endpoint,
            envd_domain,
        } => {
            mcp::serve(api, args.request_timeout, envd_endpoint, envd_domain).await?;
            return Ok(0);
        }
        VmCommand::Benchmark {
            template,
            samples,
            concurrency,
            environment,
            max_p99_ready_ms,
            require_snapshot,
        } => {
            return benchmark(
                api,
                template,
                samples,
                concurrency,
                environment,
                max_p99_ready_ms,
                require_snapshot,
            )
            .await;
        }
        VmCommand::Create {
            template,
            lifetime,
            name,
        } => {
            let mut body = json!({"templateID": template, "timeout": lifetime});
            if let Some(name) = name {
                body["metadata"] = json!({"hm.name": name});
            }
            api.request(Method::POST, &["v2", "sandboxes"], Some(body))
                .await?
        }
        VmCommand::List => api.request(Method::GET, &["sandboxes"], None).await?,
        VmCommand::Inspect { id } => api.request(Method::GET, &["sandboxes", &id], None).await?,
        VmCommand::Delete { id } => {
            api.request(Method::DELETE, &["sandboxes", &id], None)
                .await?
        }
        VmCommand::Files {
            id,
            envd_endpoint,
            envd_host,
            command,
        } => {
            transfer_file(
                &api,
                &id,
                &envd_endpoint,
                envd_host.as_deref(),
                args.request_timeout,
                command,
            )
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
        VmCommand::Domain { command } => match command {
            DomainCommand::Bind { id, domain, port } => {
                api.request(
                    Method::PUT,
                    &["sandboxes", &id, "domains", &domain],
                    Some(json!({"port": port})),
                )
                .await?
            }
            DomainCommand::List { id } => {
                api.request(Method::GET, &["sandboxes", &id, "domains"], None)
                    .await?
            }
            DomainCommand::Unbind { id, domain } => {
                api.request(
                    Method::DELETE,
                    &["sandboxes", &id, "domains", &domain],
                    None,
                )
                .await?
            }
        },
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

fn sandbox_name(value: &str) -> std::result::Result<String, String> {
    if value.is_empty()
        || value.len() > 64
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err(
            "sandbox names require 1–64 ASCII letters, digits, - _ or .; dot segments are refused"
                .into(),
        );
    }
    Ok(value.to_owned())
}

async fn tcp_stdio(tunnel: reqwest::Upgraded) -> Result<()> {
    use std::io::Read;
    use tokio::io::AsyncWriteExt;

    // A plain thread avoids Tokio stdin's uncancellable blocking task, which
    // would keep runtime shutdown waiting after the guest closes its stream.
    // Bound queued input to eight 16 KiB chunks; no unbounded buffering.
    let (sender, mut input) = tokio::sync::mpsc::channel(8);
    std::thread::Builder::new()
        .name("tcp-stdin".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            loop {
                let mut bytes = vec![0; 16 * 1024];
                match stdin.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        bytes.truncate(count);
                        if sender.blocking_send(Ok(bytes)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = sender.blocking_send(Err(error));
                        break;
                    }
                }
            }
        })
        .context("could not start stdin reader")?;
    let (mut guest_read, mut guest_write) = tokio::io::split(tunnel);
    let upload = async {
        while let Some(bytes) = input.recv().await {
            guest_write.write_all(&bytes?).await?;
        }
        // stdin EOF closes only the write direction, retaining the reply.
        guest_write.shutdown().await
    };
    let download = async {
        let mut stdout = tokio::io::stdout();
        tokio::io::copy(&mut guest_read, &mut stdout).await?;
        stdout.flush().await
    };
    tokio::pin!(upload, download);
    tokio::select! {
        result = &mut upload => {
            result.context("TCP stdin forwarding failed")?;
            download.await.context("TCP stdout forwarding failed")?;
        }
        result = &mut download => {
            // Guest EOF ends a stdio session even if local stdin remains open.
            result.context("TCP stdout forwarding failed")?;
        }
    }
    Ok(())
}

async fn tcp_forward(
    api: Api,
    id: String,
    port: u16,
    listen: std::net::SocketAddr,
    max_connections: u32,
) -> Result<()> {
    if !listen.ip().is_loopback() {
        bail!("TCP forwarding must listen on a loopback address");
    }
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .context("could not bind TCP listener")?;
    // Fail authentication and unavailable-port errors before reporting ready.
    let mut first = Some(api.tcp(&id, port).await?);
    println!(
        "{}",
        json!({"listen":listener.local_addr()?.to_string(), "sandboxID":id, "port":port})
    );
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(max_connections as usize));
    let mut connections = tokio::task::JoinSet::new();
    // Keep the signal subscription across accept/task-completion iterations.
    // Recreating ctrl_c in each select can lose a signal between receivers.
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    loop {
        tokio::select! {
            signal = &mut interrupt => {
                signal.context("could not listen for interrupt")?;
                connections.abort_all();
                while connections.join_next().await.is_some() {}
                return Ok(());
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    tracing::debug!(%error, "TCP forwarding task ended");
                }
            }
            accepted = listener.accept() => {
                let (mut client, _) = accepted.context("TCP listener failed")?;
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    drop(client);
                    continue;
                };
                let initial = first.take();
                let api = api.clone();
                let id = id.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let result: Result<()> = async {
                        let mut tunnel = match initial {
                            Some(tunnel) => tunnel,
                            None => api.tcp(&id, port).await?,
                        };
                        tokio::io::copy_bidirectional(&mut client, &mut tunnel).await?;
                        Ok(())
                    }.await;
                    if let Err(error) = result {
                        eprintln!("TCP connection ended: {error}");
                    }
                });
            }
        }
    }
}

const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

async fn transfer_file(
    api: &Api,
    id: &str,
    endpoint: &str,
    host: Option<&str>,
    timeout: u64,
    command: VmFileCommand,
) -> Result<Value> {
    use std::io::{Read, Write};
    let envd = Api::new(endpoint, timeout, None)?;
    let virtual_host = host
        .map(reqwest::header::HeaderValue::from_str)
        .transpose()
        .map_err(|_| anyhow::anyhow!("invalid envd virtual host"))?;
    let path = match &command {
        VmFileCommand::Upload { path, .. } | VmFileCommand::Download { path, .. } => path,
    };
    if path.is_empty() || path.contains('\0') {
        bail!("guest file path must be nonempty and cannot contain NUL");
    }
    let mut url = envd.base.clone();
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid envd endpoint"))?
        .pop_if_empty()
        .push("files");
    url.query_pairs_mut().append_pair("path", path);
    // Check local inputs before extending or resuming the remote sandbox.
    let upload = if let VmFileCommand::Upload { source, .. } = &command {
        let file =
            std::fs::File::open(source).with_context(|| format!("opening {}", source.display()))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("upload source must be a regular file");
        }
        if metadata.len() > MAX_FILE_BYTES {
            bail!("file exceeds envd's 512 MiB limit");
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            bail!("file exceeds envd's 512 MiB limit");
        }
        Some(bytes)
    } else {
        None
    };
    if let VmFileCommand::Download { destination, .. } = &command {
        if destination.try_exists()? {
            bail!(
                "download destination already exists: {}",
                destination.display()
            );
        }
    }
    let descriptor = api
        .request(
            Method::POST,
            &["sandboxes", id, "connect"],
            Some(json!({"timeout": 300})),
        )
        .await?;
    let token = descriptor["envdAccessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .context("connect response missing envdAccessToken")?;
    let mut token = reqwest::header::HeaderValue::from_str(token)
        .map_err(|_| anyhow::anyhow!("invalid sandbox access token"))?;
    token.set_sensitive(true);
    // This is a separate client: never forward the platform's x-api-key to envd.
    let mut request = if let Some(bytes) = upload {
        envd.client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes)
    } else {
        envd.client.get(url)
    };
    if let Some(host) = virtual_host {
        request = request.header(reqwest::header::HOST, host);
    }
    let mut response = request
        .header("x-access-token", token)
        .send()
        .await
        .context("envd file request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        bail!("envd returned {status}: {}", response.text().await?);
    }
    match command {
        VmFileCommand::Upload { .. } => response
            .json()
            .await
            .context("invalid envd upload response"),
        VmFileCommand::Download { path, destination } => {
            if response
                .content_length()
                .is_some_and(|size| size > MAX_FILE_BYTES)
            {
                bail!("file exceeds envd's 512 MiB limit");
            }
            let parent = destination
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            let mut size = 0u64;
            while let Some(chunk) = response.chunk().await? {
                size += chunk.len() as u64;
                if size > MAX_FILE_BYTES {
                    bail!("file exceeds envd's 512 MiB limit");
                }
                file.write_all(&chunk)?;
            }
            file.flush()?;
            file.persist_noclobber(&destination)
                .map_err(|error| error.error)
                .with_context(|| format!("saving {} without overwriting", destination.display()))?;
            Ok(json!({"path": path, "destination": destination, "bytes": size}))
        }
    }
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
pub(crate) fn shell_exec(command: &[String]) -> Result<String> {
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
            return json!({"index": index, "success": false, "phase": "create", "error": format!("{error:#}")})
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
            json!({"index": index, "sandbox_id": id, "success": true, "create_ms": create_ms, "exec_ms": exec_ms, "ready_ms": ready_ms})
        }
        (ready, cleanup) => {
            json!({"index": index, "sandbox_id": id, "success": false, "phase": "ready_or_cleanup", "create_ms": create_ms,
            "ready_error": ready.err().map(|e| format!("{e:#}")), "cleanup_error": cleanup.err().map(|e| format!("{e:#}"))})
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
    require_snapshot: bool,
) -> Result<i32> {
    if environment.trim().is_empty() {
        bail!("--environment must describe the test environment");
    }
    // Preparation is outside the timed lifecycle, but its state is evidence:
    // a node can fall back to cold boot when building its template fails.
    let (template_metadata, template_metadata_error) =
        match api.request(Method::GET, &["templates"], None).await {
            Ok(list) => (
                list.as_array()
                    .and_then(|entries| {
                        entries.iter().find(|entry| {
                            entry["templateID"] == template
                                || entry["aliases"].as_array().is_some_and(|aliases| {
                                    aliases.iter().any(|alias| alias == &template)
                                })
                        })
                    })
                    .cloned(),
                None,
            ),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
    if require_snapshot
        && !template_metadata
            .as_ref()
            .is_some_and(|entry| entry["snapshot"] == true)
    {
        bail!("--require-snapshot needs server confirmation of a prepared template; metadata: {template_metadata:?}; error: {template_metadata_error:?}");
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
        "client_os": std::env::consts::OS, "client_arch": std::env::consts::ARCH,
        "client_debug_assertions": cfg!(debug_assertions), "finished_at": chrono::Utc::now().to_rfc3339(),
            "template": template, "requested_samples": samples, "concurrency": concurrency,
            "template_metadata": template_metadata, "template_metadata_error": template_metadata_error,
            "require_snapshot": require_snapshot,
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
