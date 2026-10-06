//! Newline-framed MCP 2025-11-25 over stdio, backed only by the remote API.
use super::{shell_exec, Api};
#[path = "sandbox_vm_mcp_files.rs"]
mod files;
use anyhow::{bail, Result};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

const MAX_MESSAGE: u64 = 1024 * 1024;
const VERSION: &str = "2025-11-25";
const MAX_QUEUED_MESSAGES: usize = 8;

fn lifetime() -> u64 {
    300
}
fn command_timeout() -> u64 {
    60
}
fn copies() -> u32 {
    1
}
fn template() -> String {
    "base".into()
}

#[derive(Deserialize)]
#[serde(
    tag = "name",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Call {
    SandboxCreate {
        #[serde(default = "template")]
        template: String,
        #[serde(default = "lifetime")]
        lifetime: u64,
    },
    SandboxList {},
    SandboxInspect {
        id: String,
    },
    SandboxExec {
        id: String,
        command: Vec<String>,
        #[serde(default = "command_timeout")]
        timeout: u64,
    },
    SandboxPause {
        id: String,
    },
    SandboxResume {
        id: String,
        #[serde(default = "lifetime")]
        lifetime: u64,
    },
    SandboxFork {
        id: String,
        #[serde(default = "copies")]
        count: u32,
        #[serde(default = "lifetime")]
        lifetime: u64,
    },
    SandboxDelete {
        id: String,
    },
    CheckpointSave {
        id: String,
        name: String,
    },
    CheckpointList {
        id: String,
    },
    CheckpointRestore {
        id: String,
        name: String,
    },
    CheckpointDelete {
        id: String,
        name: String,
    },
}

impl Call {
    // All validation happens before making any HTTP request.
    fn request(self, deadline: u64) -> Result<(Method, Vec<String>, Option<Value>, bool)> {
        let root = |id: String| vec!["sandboxes".into(), id];
        let action = |id: String, operation: &str| vec!["sandboxes".into(), id, operation.into()];
        let checkpoint =
            |id: String, name: String| vec!["sandboxes".into(), id, "checkpoints".into(), name];
        let positive = |n: u64| -> Result<u64> {
            if n == 0 {
                bail!("value must be positive")
            }
            Ok(n)
        };
        let request = match self {
            Self::SandboxCreate { template, lifetime } => {
                if template.trim().is_empty() {
                    bail!("template is empty")
                }
                (
                    Method::POST,
                    vec!["v2".into(), "sandboxes".into()],
                    Some(json!({"templateID":template,"timeout":positive(lifetime)?})),
                    false,
                )
            }
            Self::SandboxList {} => (Method::GET, vec!["sandboxes".into()], None, false),
            Self::SandboxInspect { id } => (Method::GET, root(id), None, false),
            Self::SandboxDelete { id } => (Method::DELETE, root(id), None, false),
            Self::SandboxPause { id } => (Method::POST, action(id, "pause"), None, false),
            Self::SandboxResume { id, lifetime } => (
                Method::POST,
                action(id, "resume"),
                Some(json!({"timeout":positive(lifetime)?})),
                false,
            ),
            Self::SandboxFork {
                id,
                count,
                lifetime,
            } => {
                if !(1..=100).contains(&count) {
                    bail!("count must be 1-100")
                }
                (
                    Method::POST,
                    action(id, "fork"),
                    Some(json!({"count":count,"timeout":positive(lifetime)?})),
                    false,
                )
            }
            Self::SandboxExec {
                id,
                command,
                timeout,
            } => {
                if positive(timeout)? >= deadline {
                    bail!("command deadline must be below request deadline")
                }
                (
                    Method::POST,
                    action(id, "exec"),
                    Some(json!({"cmd":shell_exec(&command)?,"timeout_secs":timeout})),
                    true,
                )
            }
            Self::CheckpointSave { id, name } => (
                Method::POST,
                action(id, "checkpoints"),
                Some(json!({"name":name})),
                false,
            ),
            Self::CheckpointList { id } => (Method::GET, action(id, "checkpoints"), None, false),
            Self::CheckpointRestore { id, name } => {
                let mut path = checkpoint(id, name);
                path.push("restore".into());
                (Method::POST, path, None, false)
            }
            Self::CheckpointDelete { id, name } => {
                (Method::DELETE, checkpoint(id, name), None, false)
            }
        };
        for segment in &request.1 {
            if segment.is_empty() || matches!(segment.as_str(), "." | "..") {
                bail!("invalid path segment")
            }
        }
        if let Some(name) = request.2.as_ref().and_then(|v| v.get("name")) {
            if name
                .as_str()
                .is_none_or(|s| s.is_empty() || matches!(s, "." | ".."))
            {
                bail!("invalid checkpoint name")
            }
        }
        Ok(request)
    }
}

fn tools() -> Value {
    let mut result = Vec::new();
    let string = json!({"type":"string","minLength":1});
    let positive = json!({"type":"integer","minimum":1});
    for (name, description, properties, required, read_only) in [
        (
            "sandbox_create",
            "Create a remote VM sandbox",
            json!({"template":string,"lifetime":positive}),
            vec![],
            false,
        ),
        (
            "sandbox_list",
            "List remote VM sandboxes",
            json!({}),
            vec![],
            true,
        ),
        (
            "sandbox_inspect",
            "Inspect a remote sandbox without returning its access token",
            json!({"id":string}),
            vec!["id"],
            true,
        ),
        (
            "sandbox_exec",
            "Run an argv vector inside the sandbox",
            json!({"id":string,"command":{"type":"array","minItems":1,"items":{"type":"string"}},"timeout":positive}),
            vec!["id", "command"],
            false,
        ),
        (
            "sandbox_pause",
            "Pause a sandbox to disk",
            json!({"id":string}),
            vec!["id"],
            false,
        ),
        (
            "sandbox_resume",
            "Resume a paused sandbox",
            json!({"id":string,"lifetime":positive}),
            vec!["id"],
            false,
        ),
        (
            "sandbox_fork",
            "Fork a sandbox including memory",
            json!({"id":string,"count":{"type":"integer","minimum":1,"maximum":100},"lifetime":positive}),
            vec!["id"],
            false,
        ),
        (
            "sandbox_delete",
            "Destroy a sandbox",
            json!({"id":string}),
            vec!["id"],
            false,
        ),
        (
            "checkpoint_save",
            "Save an in-place checkpoint",
            json!({"id":string,"name":string}),
            vec!["id", "name"],
            false,
        ),
        (
            "checkpoint_list",
            "List sandbox checkpoints",
            json!({"id":string}),
            vec!["id"],
            true,
        ),
        (
            "checkpoint_restore",
            "Restore a checkpoint in place",
            json!({"id":string,"name":string}),
            vec!["id", "name"],
            false,
        ),
        (
            "checkpoint_delete",
            "Delete a checkpoint",
            json!({"id":string,"name":string}),
            vec!["id", "name"],
            false,
        ),
    ] {
        result.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only}}));
    }
    json!({"tools":result})
}

fn scrub_tokens(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("envdAccessToken");
            object.remove("accessToken");
            for child in object.values_mut() {
                scrub_tokens(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                scrub_tokens(child);
            }
        }
        _ => {}
    }
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn tool_failed(value: &Value, exec: bool) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["error"].is_object()))
        || (exec
            && (value["timed_out"].as_bool() == Some(true)
                || value["exit_code"].as_i64() != Some(0)
                || !value["signal"].is_null()))
}

#[derive(Clone, Default)]
pub(super) struct Session {
    negotiated: bool,
    ready: bool,
    files: Option<files::FileRoute>,
}

impl Session {
    pub(super) fn configured(
        endpoint: Option<String>,
        domain: Option<String>,
        deadline: u64,
        ca: Option<&std::path::Path>,
    ) -> Result<Self> {
        let files = endpoint
            .map(|endpoint| files::FileRoute::with_ca(&endpoint, domain, deadline, ca))
            .transpose()?;
        Ok(Self {
            files,
            ..Default::default()
        })
    }

    pub(super) async fn message(
        &mut self,
        api: &Api,
        deadline: u64,
        input: &[u8],
    ) -> Option<Value> {
        let request: Value = match serde_json::from_slice(input) {
            Ok(v) => v,
            Err(_) => return Some(error(Value::Null, -32700, "Parse error")),
        };
        let id = request.get("id").cloned();
        let valid_id = id
            .as_ref()
            .is_none_or(|v| v.is_string() || v.as_i64().is_some() || v.as_u64().is_some());
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return Some(error(Value::Null, -32600, "Invalid request"));
        };
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !valid_id {
            return Some(error(Value::Null, -32600, "Invalid request"));
        }
        if id.is_none() {
            if method == "notifications/initialized" && self.negotiated {
                self.ready = true;
            }
            // Notifications never execute tool calls or receive responses.
            return None;
        }
        let id = id.unwrap();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Some(error(id, -32602, "Invalid params"));
        }
        let result = match method {
            "initialize" => {
                if self.negotiated {
                    return Some(error(id, -32600, "Already initialized"));
                }
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(error(id, -32602, "Invalid initialize params"));
                }
                self.negotiated = true;
                json!({"protocolVersion":VERSION,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"hypermachine-sandbox","version":env!("CARGO_PKG_VERSION")}})
            }
            "ping" => json!({}),
            "tools/list" if self.ready => {
                let mut value = tools();
                if self.files.is_some() {
                    value["tools"]
                        .as_array_mut()
                        .unwrap()
                        .extend(files::tools());
                }
                value
            }
            "tools/call" if self.ready => {
                let mut call_params = params;
                // MCP request metadata is not part of the tool's argument schema.
                call_params.as_object_mut().unwrap().remove("_meta");
                if call_params.get("arguments").is_none() {
                    call_params["arguments"] = json!({});
                }
                if matches!(
                    call_params["name"].as_str(),
                    Some("file_upload" | "file_download")
                ) {
                    let Some(route) = &self.files else {
                        return Some(error(id, -32602, "File tools are not configured"));
                    };
                    let operation = serde_json::from_value::<files::FileCall>(call_params)
                        .map_err(anyhow::Error::from)
                        .and_then(files::FileCall::prepare);
                    let operation = match operation {
                        Ok(value) => value,
                        Err(_) => return Some(error(id, -32602, "Invalid file tool arguments")),
                    };
                    let result = match tokio::time::timeout(
                        std::time::Duration::from_secs(deadline),
                        route.execute(api, operation),
                    )
                    .await
                    {
                        Ok(Ok(value)) => {
                            json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
                        }
                        _ => {
                            json!({"content":[{"type":"text","text":"Sandbox file operation failed"}],"isError":true})
                        }
                    };
                    return Some(json!({"jsonrpc":"2.0","id":id,"result":result}));
                }
                let call: Call = match serde_json::from_value(call_params) {
                    Ok(call) => call,
                    Err(_) => return Some(error(id, -32602, "Unknown tool or invalid arguments")),
                };
                let (method, path, body, exec) = match call.request(deadline) {
                    Ok(request) => request,
                    Err(_) => return Some(error(id, -32602, "Invalid tool arguments")),
                };
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                match api.request(method, &path, body).await {
                    Ok(mut value) => {
                        scrub_tokens(&mut value);
                        let failed = tool_failed(&value, exec);
                        json!({"content":[{"type":"text","text":value.to_string()}],"isError":failed})
                    }
                    // Do not echo an upstream response body that might contain credentials.
                    Err(_) => {
                        json!({"content":[{"type":"text","text":"Sandbox API operation failed"}],"isError":true})
                    }
                }
            }
            "tools/list" | "tools/call" => {
                return Some(error(id, -32002, "Initialization required"))
            }
            _ => return Some(error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}

fn request_id(input: &[u8]) -> Option<Value> {
    let value: Value = serde_json::from_slice(input).ok()?;
    if value["jsonrpc"] != "2.0" || value["method"].as_str()? == "initialize" {
        return None;
    }
    let id = value.get("id")?;
    (id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()).then(|| id.clone())
}

pub(super) fn cancellation_id(input: &[u8]) -> Option<Value> {
    let value: Value = serde_json::from_slice(input).ok()?;
    if value["jsonrpc"] != "2.0"
        || value["method"] != "notifications/cancelled"
        || value.get("id").is_some()
        || value["params"]
            .get("reason")
            .is_some_and(|reason| !reason.is_string())
    {
        return None;
    }
    let id = value["params"].get("requestId")?;
    (id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()).then(|| id.clone())
}

async fn read_frame<R: AsyncBufRead + Unpin>(
    input: &mut R,
    partial: &mut Vec<u8>,
) -> Result<Option<Vec<u8>>> {
    // read_until preserves consumed bytes in `partial` if select drops its
    // future. Bound the cumulative frame, rather than each read invocation.
    input
        .take(MAX_MESSAGE + 1 - partial.len() as u64)
        .read_until(b'\n', partial)
        .await?;
    if partial.len() as u64 > MAX_MESSAGE {
        bail!("MCP message exceeds 1 MiB limit")
    }
    Ok((!partial.is_empty()).then(|| std::mem::take(partial)))
}

async fn transport<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    api: &Api,
    deadline: u64,
    files: Option<files::FileRoute>,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let mut session = Session {
        files,
        ..Default::default()
    };
    let mut partial = Vec::new();
    let mut queued: VecDeque<Vec<u8>> = VecDeque::new();
    loop {
        let line = match queued.pop_front() {
            Some(line) => line,
            None => match read_frame(input, &mut partial).await? {
                Some(line) => line,
                None => return Ok(()),
            },
        };
        let active_id = request_id(&line);
        let response = {
            let operation = session.message(api, deadline, &line);
            tokio::pin!(operation);
            loop {
                tokio::select! {
                    // Finish an already-ready response before considering a
                    // cancellation that raced with completion.
                    biased;
                    response = &mut operation => break response,
                    frame = read_frame(input, &mut partial) => {
                        let Some(frame) = frame? else { return Ok(()); };
                        if let Some(id) = cancellation_id(&frame) {
                            if active_id.as_ref() == Some(&id) {
                                // Dropping the HTTP future releases this client
                                // wait; remote effects are not rolled back.
                                break None;
                            }
                            queued.retain(|frame| request_id(frame).as_ref() != Some(&id));
                            continue;
                        }
                        if queued.len() >= MAX_QUEUED_MESSAGES
                            || queued.iter().map(Vec::len).sum::<usize>() + frame.len() > MAX_MESSAGE as usize
                        {
                            bail!("MCP pending input exceeds queue limit")
                        }
                        queued.push_back(frame);
                    }
                }
            }
        };
        if let Some(response) = response {
            output
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
}

pub(super) async fn serve(
    api: Api,
    deadline: u64,
    endpoint: Option<String>,
    domain: Option<String>,
) -> Result<()> {
    let files = endpoint
        .map(|endpoint| files::FileRoute::new(&endpoint, domain, deadline))
        .transpose()?;
    transport(
        &api,
        deadline,
        files,
        &mut BufReader::new(tokio::io::stdin()),
        &mut tokio::io::stdout(),
    )
    .await
}

#[cfg(test)]
#[path = "sandbox_vm_mcp_transport_tests.rs"]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> Api {
        Api::new("http://127.0.0.1:1", 1, None).unwrap()
    }
    fn initialize() -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})
    }
    async fn message(session: &mut Session, value: Value) -> Value {
        session
            .message(&api(), 120, &serde_json::to_vec(&value).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn initialization_and_notifications_gate_tool_discovery() {
        let mut session = Session::default();
        assert_eq!(
            message(
                &mut session,
                json!({"jsonrpc":"2.0","id":"early","method":"tools/list"})
            )
            .await["error"]["code"],
            -32002
        );
        assert_eq!(
            message(&mut session, initialize()).await["result"]["protocolVersion"],
            VERSION
        );
        assert!(!session.ready);
        assert!(session
            .message(
                &api(),
                120,
                br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .await
            .is_none());
        assert_eq!(
            message(
                &mut session,
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
            )
            .await["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        assert!(session
            .message(
                &api(),
                120,
                br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"sandbox_create"}}"#
            )
            .await
            .is_none());
    }

    #[tokio::test]
    async fn invalid_arguments_never_reach_the_api() {
        let mut session = Session {
            negotiated: true,
            ready: true,
            files: None,
        };
        for (name, arguments) in [
            ("sandbox_create", json!({"lifetime":0})),
            ("sandbox_fork", json!({"id":"vm","count":101})),
            ("sandbox_exec", json!({"id":"vm","command":[]})),
            (
                "sandbox_exec",
                json!({"id":"vm","command":["true"],"timeout":120}),
            ),
            ("sandbox_inspect", json!({"id":".."})),
            ("checkpoint_save", json!({"id":"vm","name":""})),
            ("sandbox_list", json!({"unexpected":true})),
        ] {
            let response=message(&mut session,json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments,"_meta":{}}})).await;
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }
    }

    #[tokio::test]
    async fn parse_errors_and_invalid_ids_are_protocol_errors() {
        let mut session = Session::default();
        assert_eq!(
            session.message(&api(), 120, b"{bad").await.unwrap()["error"]["code"],
            -32700
        );
        assert_eq!(
            message(
                &mut session,
                json!({"jsonrpc":"2.0","id":null,"method":"ping"})
            )
            .await["error"]["code"],
            -32600
        );
        assert_eq!(
            message(
                &mut session,
                json!({"jsonrpc":"2.0","id":1.5,"method":"ping"})
            )
            .await["error"]["code"],
            -32600
        );
    }

    #[tokio::test]
    async fn transport_bounds_messages_without_a_newline() {
        let bytes = vec![b'x'; (MAX_MESSAGE + 1) as usize];
        let mut input = BufReader::new(bytes.as_slice());
        let mut output = Vec::new();
        assert!(transport(&api(), 120, None, &mut input, &mut output)
            .await
            .is_err());
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn file_tools_require_operator_configuration_and_validate_before_connect() {
        let mut session = Session {
            negotiated: true,
            ready: true,
            files: None,
        };
        let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
        assert_eq!(
            message(&mut session, list.clone()).await["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        let call = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"file_download","arguments":{"id":"vm","path":"/file"}}});
        assert_eq!(
            message(&mut session, call.clone()).await["error"]["code"],
            -32602
        );
        session.files = Some(files::FileRoute::new("http://127.0.0.1:1", None, 1).unwrap());
        assert_eq!(
            message(&mut session, list).await["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            14
        );
        let mut invalid = call;
        invalid["params"]["arguments"]["endpoint"] = json!("http://attacker");
        assert_eq!(
            message(&mut session, invalid).await["error"]["code"],
            -32602
        );
    }

    #[test]
    fn platform_tokens_are_removed_from_nested_results() {
        let mut value = json!({"envdAccessToken":"secret","sandboxes":[{"accessToken":"secret","sandboxID":"vm"}]});
        scrub_tokens(&mut value);
        assert_eq!(value, json!({"sandboxes":[{"sandboxID":"vm"}]}));
    }

    #[test]
    fn partial_forks_and_incomplete_guest_results_are_errors() {
        assert!(tool_failed(
            &json!([{"sandbox":{"sandboxID":"child"}},{"error":{"code":503}}]),
            false
        ));
        assert!(!tool_failed(
            &json!([{"sandbox":{"sandboxID":"child"}}]),
            false
        ));
        assert!(tool_failed(&json!({"stdout":"","exit_code":null}), true));
        assert!(tool_failed(&json!({"exit_code":7}), true));
        assert!(!tool_failed(&json!({"exit_code":0}), true));
    }
}
