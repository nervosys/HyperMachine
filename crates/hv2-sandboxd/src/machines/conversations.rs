//! Conversations: a coding agent talked to over many turns, steered while it
//! works, and handed from one agent to another.
//!
//! A [prompt](super::prompts) is one turn: a caller who wants a second one
//! carries the agent's session over by hand. A conversation does that for
//! them, and does what a single prompt cannot:
//!
//! - `POST /machines/{name}/conversations` `{"provider", "name", "model",
//!   "user", "cwd", "unattended", "from"}` makes one. Nothing runs yet.
//! - `POST .../conversations/{id}/messages` `{"prompt", "env"}` says
//!   something to it. With nothing running, that starts a turn, carrying on
//!   the agent's session. With a turn of Claude Code's running, the message
//!   goes into that turn: it **steers** it.
//! - `GET  .../conversations` and `.../conversations/{id}` list and describe
//!   them: their turns, which are prompts, and every message sent.
//! - `GET  .../conversations/{id}/transcript` is what was said, by whom, in
//!   order: the caller's messages and the agent's replies, read from the
//!   agent's own events.
//! - `POST .../conversations/{id}/interrupt` interrupts the turn running.
//! - `DELETE .../conversations/{id}` forgets one that is not running.
//!
//! `{id}` is the conversation's number, or the name it was given.
//!
//! # Steering
//!
//! A turn of Claude Code's is run with its input as a stream of JSON lines,
//! one a message (`--input-format stream-json`). The first message starts
//! it; one sent while it runs is written to the same process, which takes it
//! as the caller's next words, in the same session. Its standard input is
//! closed, and it exits, once it has answered every message it was sent,
//! each with a `result` event.
//!
//! How it fits a message into work already going is Claude Code's own. That
//! it is in the same process and session, and is answered, was checked with
//! the real harness and no key. What a model does with it has not been.
//!
//! Codex is run a turn at a time, `codex exec resume` carrying its thread on.
//! It has no input to steer through while it runs, so a message then is
//! refused: interrupt the turn, or wait for it to end.
//!
//! # Moving to another agent
//!
//! `from` names a conversation, of this machine, to carry on from. Its
//! transcript, the newest [`MAX_CARRIED`] bytes of it, is put before the
//! first message of the new one, which may be with another provider. A
//! harness has no way in for another harness's session, so what moves is
//! what was said, not what the agent remembers.
//!
//! # What is kept
//!
//! On the node, in memory, as prompts and runs are: a conversation is gone
//! when the node restarts or its machine is deleted, and a turn whose run
//! made room for newer ones is left out of its transcript. The harness's own
//! session, in the guest, is the guest's. The `env` of a message, where its
//! key goes, is a turn's environment and is not kept; a message that steers
//! a running turn has the environment that turn was started with.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use axum::http::HeaderMap;
use serde_json::{json, Value};

use super::super::{
    api_error, now_ms, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State,
    StatusCode,
};
use super::{named, prompts, runs, AgentVM};

/// The most conversations kept for one machine.
pub(crate) const MAX_CONVERSATIONS: usize = 64;

/// The most of another conversation's transcript carried into a new one.
pub(crate) const MAX_CARRIED: usize = 64 << 10;

/// The providers a conversation can be with.
const PROVIDERS: [&str; 2] = ["claude", "codex"];

/// A message the caller sent, and the turn it went to.
struct Message {
    prompt: String,
    turn: u64,
    /// Into a turn that was running, rather than starting one.
    steered: bool,
    sent_ms: u64,
}

/// A turn of Claude Code's whose standard input is still open.
struct Open {
    turn: u64,
    /// Messages written to it, each answered with a `result`.
    sent: u64,
    /// Every message has been answered and its input is being closed: a
    /// message now starts a turn of its own.
    closing: bool,
}

struct Conversation {
    id: u64,
    name: Option<String>,
    provider: String,
    model: Option<String>,
    user: Option<String>,
    cwd: Option<String>,
    unattended: bool,
    created_ms: u64,
    /// The conversation this one carries on from, and what of it is put
    /// before its first message.
    from: Option<u64>,
    carried: Option<String>,
    turns: Vec<u64>,
    messages: Vec<Message>,
    open: Option<Open>,
    /// A message is starting a turn: another waits for it.
    starting: bool,
}

#[derive(Default)]
struct MachineConversations {
    last_id: u64,
    by_id: BTreeMap<u64, Conversation>,
}

/// Every machine's conversations on this node, by machine ID.
static CONVERSATIONS: StdMutex<Option<HashMap<String, MachineConversations>>> = StdMutex::new(None);

fn conversations<T>(f: impl FnOnce(&mut HashMap<String, MachineConversations>) -> T) -> T {
    let mut guard = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Forget a machine's conversations, when it is deleted.
pub(crate) fn forget(machine_id: &str) {
    conversations(|all| all.remove(machine_id));
}

/// The conversation `key` names, by its number or its name.
fn find<'a>(kept: &'a mut MachineConversations, key: &str) -> Option<&'a mut Conversation> {
    match key.parse::<u64>() {
        Ok(id) => kept.by_id.get_mut(&id),
        Err(_) => kept
            .by_id
            .values_mut()
            .find(|c| c.name.as_deref() == Some(key)),
    }
}

/// Whether the machine's run `id` is still going.
fn is_running(machine_id: &str, id: u64) -> bool {
    runs::with_run(machine_id, id, runs::Run::is_running).unwrap_or(false)
}

/// Every event of the machine's run `id`, or none once it is forgotten.
fn events_of(machine_id: &str, id: u64) -> Vec<Value> {
    runs::with_run(machine_id, id, |run| {
        let (offset, text, _) = run.stdout_from(0);
        prompts::events(text, offset, !run.is_running()).0
    })
    .unwrap_or_default()
}

/// The text of a message's content: a string, or the `text` of its parts.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// What was said in one turn's events: `(role, text)`, in order. Claude
/// Code says the caller's messages back, so both sides are in its events;
/// Codex says only its own, and the caller's are `messages`.
fn said(provider: &str, events: &[Value], messages: &[&str]) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    let mut heard_user = false;
    for entry in events {
        let Some(event) = entry.get("event") else {
            continue;
        };
        let kind = event.get("type").and_then(Value::as_str);
        let (role, text) = match (provider, kind) {
            ("claude", Some("user")) => {
                // A tool's result comes back as a user message too; only a
                // message of the caller's, said back, is the caller's.
                if event.get("isReplay").and_then(Value::as_bool) != Some(true) {
                    continue;
                }
                heard_user = true;
                ("user", text_of(&event["message"]["content"]))
            }
            ("claude", Some("assistant")) => ("assistant", text_of(&event["message"]["content"])),
            ("codex", Some("item.completed"))
                if event["item"]["type"].as_str() == Some("agent_message") =>
            {
                (
                    "assistant",
                    event["item"]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                )
            }
            _ => continue,
        };
        if !text.trim().is_empty() {
            found.push((role, text));
        }
    }
    if !heard_user {
        // Before the agent's words, as they were.
        let mut before: Vec<(&'static str, String)> = messages
            .iter()
            .map(|m| ("user", (*m).to_string()))
            .collect();
        before.append(&mut found);
        found = before;
    }
    found
}

/// What was said in the conversation, turn by turn.
fn transcript(machine_id: &str, conversation: &Conversation) -> Vec<Value> {
    let mut lines = Vec::new();
    for &turn in &conversation.turns {
        let messages: Vec<&str> = conversation
            .messages
            .iter()
            .filter(|m| m.turn == turn)
            .map(|m| m.prompt.as_str())
            .collect();
        for (role, text) in said(
            &conversation.provider,
            &events_of(machine_id, turn),
            &messages,
        ) {
            lines.push(json!({ "role": role, "text": text, "promptID": turn }));
        }
    }
    lines
}

/// The transcript as text for another agent to read, its newest
/// [`MAX_CARRIED`] bytes.
fn carried_text(provider: &str, lines: &[Value]) -> String {
    let mut said = String::new();
    for line in lines {
        let who = if line["role"] == "user" {
            "User"
        } else {
            "Assistant"
        };
        said.push_str(&format!(
            "{who}: {}\n\n",
            line["text"].as_str().unwrap_or("")
        ));
    }
    if said.len() > MAX_CARRIED {
        let mut cut = said.len() - MAX_CARRIED;
        while !said.is_char_boundary(cut) {
            cut += 1;
        }
        said = format!("[earlier turns left out]\n\n{}", &said[cut..]);
    }
    format!(
        "This conversation carries on from one with another agent ({provider}). What was \
         said there:\n\n{said}That is the end of it. The user's next message follows.\n\n"
    )
}

/// The agent's session, from the newest turn that reported one.
fn session(machine_id: &str, conversation: &Conversation) -> Option<String> {
    conversation
        .turns
        .iter()
        .rev()
        .find_map(|&turn| prompts::session_in(&events_of(machine_id, turn)))
}

/// The turn running, if one is.
fn running_turn(machine_id: &str, conversation: &Conversation) -> Option<u64> {
    conversation
        .turns
        .last()
        .copied()
        .filter(|&turn| is_running(machine_id, turn))
}

fn describe(machine_id: &str, conversation: &Conversation, full: bool) -> Value {
    let running = running_turn(machine_id, conversation);
    let mut described = json!({
        "conversationID": conversation.id,
        "name": conversation.name,
        "provider": conversation.provider,
        "model": conversation.model,
        "user": conversation.user,
        "cwd": conversation.cwd,
        "unattended": conversation.unattended,
        "from": conversation.from,
        "createdAt": conversation.created_ms,
        "state": if running.is_some() { "running" } else { "idle" },
        "runningPromptID": running,
        "turns": conversation.turns,
        "messageCount": conversation.messages.len(),
    });
    if full {
        described["session"] = json!(session(machine_id, conversation));
        described["messages"] = conversation
            .messages
            .iter()
            .map(|m| {
                json!({
                    "prompt": m.prompt,
                    "promptID": m.turn,
                    "steered": m.steered,
                    "sentAt": m.sent_ms,
                })
            })
            .collect();
    }
    described
}

/// A message as Claude Code reads it on its standard input: one line.
fn message_line(prompt: &str) -> String {
    let mut line = json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": prompt}]},
    })
    .to_string();
    line.push('\n');
    line
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.parse::<u64>().is_err()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewConversation {
    provider: String,
    name: Option<String>,
    model: Option<String>,
    user: Option<String>,
    cwd: Option<String>,
    #[serde(default)]
    unattended: bool,
    /// A conversation of this machine's to carry on from, by number or name.
    from: Option<String>,
}

/// `POST /machines/{name}/conversations`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<NewConversation>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    if !PROVIDERS.contains(&req.provider.as_str()) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "no provider {:?} for a conversation: claude or codex",
                req.provider
            ),
        );
    }
    if req.model.as_ref().is_some_and(|m| m.trim().is_empty()) {
        return api_error(StatusCode::BAD_REQUEST, "model is empty");
    }
    if let Some(why) = prompts::refused_as_root(&req.provider, req.unattended, req.user.as_deref())
    {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    if let Some(given) = &req.name {
        if !valid_name(given) {
            return api_error(
                StatusCode::BAD_REQUEST,
                "a conversation's name is letters, digits, - and _, up to 63, and not a number",
            );
        }
    }
    let machine_id = machine.id.clone();
    // The error is the reply, returned by the handler at once.
    #[allow(clippy::result_large_err)]
    let made = conversations(|all| {
        let kept = all.entry(machine_id.clone()).or_default();
        if kept.by_id.len() >= MAX_CONVERSATIONS {
            return Err(api_error(
                StatusCode::CONFLICT,
                format!(
                    "machine {name} has {MAX_CONVERSATIONS} conversations, the most it keeps; \
                     delete one"
                ),
            ));
        }
        if let Some(given) = &req.name {
            if kept.by_id.values().any(|c| c.name.as_ref() == Some(given)) {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    format!("machine {name} has a conversation named {given}"),
                ));
            }
        }
        let (from, carried) = match &req.from {
            None => (None, None),
            Some(key) => match find(kept, key) {
                Some(source) => (
                    Some(source.id),
                    Some(carried_text(
                        &source.provider,
                        &transcript(&machine_id, source),
                    )),
                ),
                None => {
                    return Err(api_error(
                        StatusCode::NOT_FOUND,
                        format!("machine {name} has no conversation {key} to carry on from"),
                    ))
                }
            },
        };
        kept.last_id += 1;
        let conversation = Conversation {
            id: kept.last_id,
            name: req.name.clone(),
            provider: req.provider.clone(),
            model: req.model.clone(),
            user: req.user.clone(),
            cwd: req.cwd.clone(),
            unattended: req.unattended,
            created_ms: now_ms(),
            from,
            carried,
            turns: Vec::new(),
            messages: Vec::new(),
            open: None,
            starting: false,
        };
        let described = describe(&machine_id, &conversation, true);
        kept.by_id.insert(conversation.id, conversation);
        Ok(described)
    });
    match made {
        Ok(described) => (StatusCode::CREATED, Json(described)).into_response(),
        Err(response) => response,
    }
}

/// `GET /machines/{name}/conversations`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let listed: Vec<Value> = conversations(|all| {
        all.get(&machine.id)
            .map(|kept| {
                kept.by_id
                    .values()
                    .map(|c| describe(&machine.id, c, false))
                    .collect()
            })
            .unwrap_or_default()
    });
    Json(listed).into_response()
}

fn no_such(name: &str, key: &str) -> Response {
    api_error(
        StatusCode::NOT_FOUND,
        format!("machine {name} has no conversation {key}"),
    )
}

/// `GET /machines/{name}/conversations/{id}`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let found = conversations(|all| {
        let kept = all.get_mut(&machine.id)?;
        find(kept, &key).map(|c| describe(&machine.id, c, true))
    });
    match found {
        Some(described) => Json(described).into_response(),
        None => no_such(&name, &key),
    }
}

/// `GET /machines/{name}/conversations/{id}/transcript`.
pub(crate) async fn get_transcript(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let found = conversations(|all| {
        let kept = all.get_mut(&machine.id)?;
        find(kept, &key).map(|c| transcript(&machine.id, c))
    });
    match found {
        Some(lines) => Json(lines).into_response(),
        None => no_such(&name, &key),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewMessage {
    prompt: String,
    /// The agent's key and anything else it reads from its environment, for
    /// a turn this message starts. Not kept, and not returned.
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// What a message does, decided with the conversation in hand.
enum Delivery {
    /// Into the open turn of Claude Code's, `turn`.
    Steer { id: u64, turn: u64 },
    /// A turn of its own, with what it needs to start one.
    Start {
        id: u64,
        provider: String,
        model: Option<String>,
        user: Option<String>,
        cwd: Option<String>,
        unattended: bool,
        session: Option<String>,
        carried: Option<String>,
    },
}

/// `POST /machines/{name}/conversations/{id}/messages`.
pub(crate) async fn message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
    Json(req): Json<NewMessage>,
) -> Response {
    if req.prompt.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "prompt is empty");
    }
    if req.prompt.len() > prompts::MAX_PROMPT {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("prompt is at most {} KiB", prompts::MAX_PROMPT / 1024),
        );
    }
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let machine_id = machine.id.clone();
    // The error is the reply, returned by the handler at once.
    #[allow(clippy::result_large_err)]
    let decided = conversations(|all| {
        let Some(conversation) = all.get_mut(&machine_id).and_then(|kept| find(kept, &key)) else {
            return Err(no_such(&name, &key));
        };
        if conversation.starting {
            return Err(api_error(
                StatusCode::CONFLICT,
                "a message is starting a turn of this conversation; send this one when it has",
            ));
        }
        let running = running_turn(&machine_id, conversation);
        if let (Some(turn), Some(open)) = (running, conversation.open.as_mut()) {
            if open.turn == turn && !open.closing {
                open.sent += 1;
                conversation.messages.push(Message {
                    prompt: req.prompt.clone(),
                    turn,
                    steered: true,
                    sent_ms: now_ms(),
                });
                return Ok(Delivery::Steer {
                    id: conversation.id,
                    turn,
                });
            }
        }
        if let Some(turn) = running {
            // Claude Code's turn that has answered everything and is closing
            // takes no more; a new turn carries the session on. Codex's
            // cannot be spoken to while it runs.
            if conversation.provider != "claude" {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    format!(
                        "turn {turn} of this conversation is running, and codex takes no \
                         message while it does; interrupt it, or send this when it ends"
                    ),
                ));
            }
        }
        conversation.starting = true;
        Ok(Delivery::Start {
            id: conversation.id,
            provider: conversation.provider.clone(),
            model: conversation.model.clone(),
            user: conversation.user.clone(),
            cwd: conversation.cwd.clone(),
            unattended: conversation.unattended,
            session: session(&machine_id, conversation),
            carried: conversation
                .turns
                .is_empty()
                .then(|| conversation.carried.clone())
                .flatten(),
        })
    });
    match decided {
        Err(response) => response,
        Ok(Delivery::Steer { id, turn }) => {
            steer(&state, &headers, &name, &machine_id, id, turn, &req.prompt).await
        }
        Ok(Delivery::Start {
            id,
            provider,
            model,
            user,
            cwd,
            unattended,
            session,
            carried,
        }) => {
            let started = start_turn(
                &state,
                &headers,
                &name,
                &machine_id,
                id,
                Turn {
                    provider,
                    model,
                    user,
                    cwd,
                    unattended,
                    session,
                    prompt: req.prompt,
                    carried,
                    env: req.env,
                },
            )
            .await;
            conversations(|all| {
                if let Some(c) = all.get_mut(&machine_id).and_then(|k| k.by_id.get_mut(&id)) {
                    c.starting = false;
                }
            });
            started
        }
    }
}

/// Write a message into the open turn `turn`.
async fn steer(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    machine_id: &str,
    id: u64,
    turn: u64,
    prompt: &str,
) -> Response {
    let written = match runs::reachable(state, headers, name, turn) {
        Ok((vm, pid)) => runs::write_stdin(&vm, pid, &message_line(prompt), false)
            .await
            .map_err(|e| {
                api_error(
                    StatusCode::BAD_GATEWAY,
                    format!("writing to turn {turn}: {e}"),
                )
            }),
        Err(response) => Err(response),
    };
    if let Err(response) = written {
        // Not delivered: not counted, and not kept as sent.
        conversations(|all| {
            if let Some(c) = all.get_mut(machine_id).and_then(|k| k.by_id.get_mut(&id)) {
                if let Some(open) = c.open.as_mut().filter(|o| o.turn == turn) {
                    open.sent = open.sent.saturating_sub(1);
                }
                if let Some(at) = c
                    .messages
                    .iter()
                    .rposition(|m| m.turn == turn && m.steered && m.prompt == prompt)
                {
                    c.messages.remove(at);
                }
            }
        });
        return response;
    }
    (
        StatusCode::ACCEPTED,
        Json(json!({ "conversationID": id, "promptID": turn, "steered": true })),
    )
        .into_response()
}

/// What starting a turn needs.
struct Turn {
    provider: String,
    model: Option<String>,
    user: Option<String>,
    cwd: Option<String>,
    unattended: bool,
    session: Option<String>,
    prompt: String,
    carried: Option<String>,
    env: BTreeMap<String, String>,
}

async fn start_turn(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    name: &str,
    machine_id: &str,
    id: u64,
    turn: Turn,
) -> Response {
    let streamed = turn.provider == "claude";
    let choices = prompts::Choices {
        model: turn.model.is_some(),
        resume: turn.session.is_some(),
        unattended: turn.unattended,
        streamed,
    };
    let cmd = match prompts::command_for(&turn.provider, choices, None) {
        Ok(cmd) => cmd,
        Err(why) => return api_error(StatusCode::BAD_REQUEST, why),
    };
    // What the agent is told: the conversation carried over, then this.
    let told = match &turn.carried {
        Some(carried) => format!("{carried}{}", turn.prompt),
        None => turn.prompt.clone(),
    };
    let mut env = turn.env;
    prompts::keep_as_installed(&turn.provider, &mut env);
    if !streamed {
        env.insert(prompts::PROMPT_VAR.to_string(), told.clone());
    }
    if let Some(model) = &turn.model {
        env.insert(prompts::MODEL_VAR.to_string(), model.clone());
    }
    if let Some(session) = &turn.session {
        env.insert(prompts::RESUME_VAR.to_string(), session.clone());
    }
    let run = runs::NewRun {
        cmd,
        cwd: turn.cwd,
        env,
        user: turn.user,
        // Claude Code's is left open, for messages while it works; Codex
        // must not sit waiting on one.
        stdin: (!streamed).then(String::new),
    };
    let (_, run_id, _) = match runs::launch(state, headers, name, run).await {
        Ok(started) => started,
        Err(response) => return response,
    };
    prompts::remember(
        machine_id.to_string(),
        run_id,
        prompts::Prompt {
            provider: turn.provider.clone(),
            prompt: turn.prompt.clone(),
            model: turn.model,
            resume: turn.session,
        },
    );
    conversations(|all| {
        if let Some(c) = all.get_mut(machine_id).and_then(|k| k.by_id.get_mut(&id)) {
            c.turns.push(run_id);
            c.messages.push(Message {
                prompt: turn.prompt.clone(),
                turn: run_id,
                steered: false,
                sent_ms: now_ms(),
            });
            if streamed {
                c.open = Some(Open {
                    turn: run_id,
                    sent: 1,
                    closing: false,
                });
            }
        }
    });
    if streamed {
        let written = match runs::reachable(state, headers, name, run_id) {
            Ok((vm, pid)) => {
                let written = runs::write_stdin(&vm, pid, &message_line(&told), false).await;
                tokio::spawn(close_when_answered(
                    machine_id.to_string(),
                    id,
                    run_id,
                    vm,
                    pid,
                ));
                written.map_err(|e| e.to_string())
            }
            // Ended already: it says why itself.
            Err(_) => Ok(()),
        };
        if let Err(e) = written {
            return api_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "turn {run_id} started, and its message could not be written: {e}{}",
                    why_it_ended(machine_id, run_id).await
                ),
            );
        }
    }
    (
        StatusCode::CREATED,
        Json(json!({ "conversationID": id, "promptID": run_id, "steered": false })),
    )
        .into_response()
}

/// What a turn that ended before it could be written to said as it went,
/// for the reply that says it could not be: its exit and the end of its
/// standard error, once it is seen to have ended, or nothing after a few
/// seconds of it not being seen to.
async fn why_it_ended(machine_id: &str, turn: u64) -> String {
    for _ in 0..20 {
        let ended = runs::with_run(machine_id, turn, |run| {
            (!run.is_running()).then(|| {
                let described = run.describe();
                format!(
                    "; the agent ended (exit code {}, signal {}) saying: {:?}",
                    described["exitCode"],
                    described["signal"],
                    run.stderr_tail(1024).trim()
                )
            })
        });
        match ended {
            Some(Some(why)) => return why,
            Some(None) => tokio::time::sleep(Duration::from_millis(150)).await,
            None => break,
        }
    }
    String::new()
}

/// Close the standard input of Claude Code's turn once it has answered every
/// message written to it, so that it ends; until then, a message is written
/// into it.
async fn close_when_answered(machine_id: String, id: u64, turn: u64, vm: Arc<AgentVM>, pid: u32) {
    let (mut read_to, mut answered) = (0u64, 0u64);
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let seen = runs::with_run(&machine_id, turn, |run| {
            let (offset, text, _) = run.stdout_from(read_to);
            let (found, next) = prompts::events(text, offset, false);
            let results = found
                .iter()
                .filter(|e| e["event"]["type"].as_str() == Some("result"))
                .count() as u64;
            (run.is_running(), results, next)
        });
        let Some((running, results, next)) = seen else {
            return;
        };
        read_to = next;
        answered += results;
        let close = conversations(|all| {
            let Some(c) = all.get_mut(&machine_id).and_then(|k| k.by_id.get_mut(&id)) else {
                // Forgotten: nothing more will be written to it.
                return true;
            };
            let Some(open) = c.open.as_mut().filter(|o| o.turn == turn) else {
                return !running;
            };
            if !running {
                c.open = None;
                return false;
            }
            if !open.closing && answered >= open.sent {
                open.closing = true;
                return true;
            }
            false
        });
        if !running {
            return;
        }
        if close {
            // An error is a turn already gone, which the next poll sees.
            let _ = runs::write_stdin(&vm, pid, "", true).await;
        }
    }
}

/// `POST /machines/{name}/conversations/{id}/interrupt`.
pub(crate) async fn interrupt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let found = conversations(|all| {
        let c = find(all.get_mut(&machine.id)?, &key)?;
        Some(running_turn(&machine.id, c))
    });
    let turn = match found {
        None => return no_such(&name, &key),
        Some(None) => {
            return api_error(
                StatusCode::CONFLICT,
                format!("conversation {key} has no turn running"),
            )
        }
        Some(Some(turn)) => turn,
    };
    const SIGINT: i32 = 2;
    match runs::signal_run(&state, &headers, &name, turn, SIGINT).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

/// `DELETE /machines/{name}/conversations/{id}`.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    conversations(|all| {
        let Some(kept) = all.get_mut(&machine.id) else {
            return no_such(&name, &key);
        };
        let Some(c) = find(kept, &key) else {
            return no_such(&name, &key);
        };
        if c.starting || running_turn(&machine.id, c).is_some() {
            return api_error(
                StatusCode::CONFLICT,
                format!("conversation {key} has a turn running; interrupt it first"),
            );
        }
        let id = c.id;
        kept.by_id.remove(&id);
        StatusCode::NO_CONTENT.into_response()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(value: Value) -> Value {
        json!({ "offset": 0, "event": value })
    }

    #[test]
    fn claude_says_both_sides_and_only_the_caller_s_messages_are_theirs() {
        let events = vec![
            event(json!({"type": "system", "subtype": "init", "session_id": "s"})),
            event(json!({"type": "user", "isReplay": true,
                         "message": {"role": "user", "content": [{"type": "text", "text": "fix it"}]}})),
            event(json!({"type": "assistant",
                         "message": {"content": [{"type": "text", "text": "Looking."},
                                                 {"type": "tool_use", "name": "Bash"}]}})),
            // A tool's result is a user message, and not the caller's.
            event(json!({"type": "user",
                         "message": {"content": [{"type": "tool_result", "content": "ok"}]}})),
            event(json!({"type": "user", "isReplay": true,
                         "message": {"role": "user", "content": "and the docs"}})),
            event(
                json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Done."}]}}),
            ),
            event(json!({"type": "result", "result": "Done."})),
            json!({"offset": 9, "text": "not json"}),
        ];
        let said = said("claude", &events, &["fix it", "and the docs"]);
        assert_eq!(
            said,
            vec![
                ("user", "fix it".to_string()),
                ("assistant", "Looking.".to_string()),
                ("user", "and the docs".to_string()),
                ("assistant", "Done.".to_string()),
            ]
        );
    }

    #[test]
    fn codex_says_only_its_own_and_the_caller_s_come_first() {
        let events = vec![
            event(json!({"type": "thread.started", "thread_id": "t"})),
            event(json!({"type": "item.completed", "item": {"type": "reasoning", "text": "hmm"}})),
            event(
                json!({"type": "item.completed", "item": {"type": "agent_message", "text": "Hello."}}),
            ),
            event(json!({"type": "turn.failed", "error": {"message": "401"}})),
        ];
        assert_eq!(
            said("codex", &events, &["hi"]),
            vec![
                ("user", "hi".to_string()),
                ("assistant", "Hello.".to_string())
            ]
        );
        // A turn that said nothing still has what the caller said.
        assert_eq!(
            said("codex", &[], &["hi"]),
            vec![("user", "hi".to_string())]
        );
    }

    #[test]
    fn a_message_is_one_line_of_json_whatever_it_holds() {
        let line = message_line("two\nlines with \"quotes\" and $(no shell)");
        assert!(
            line.ends_with('\n') && line.matches('\n').count() == 1,
            "{line}"
        );
        let back: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(back["type"], "user");
        assert_eq!(
            back["message"]["content"][0]["text"],
            "two\nlines with \"quotes\" and $(no shell)"
        );
    }

    #[test]
    fn what_is_carried_names_who_said_it_and_keeps_the_newest() {
        let lines = vec![
            json!({"role": "user", "text": "fix it"}),
            json!({"role": "assistant", "text": "Done."}),
        ];
        let carried = carried_text("claude", &lines);
        assert!(carried.contains("another agent (claude)"), "{carried}");
        assert!(
            carried.contains("User: fix it\n\nAssistant: Done.\n\n"),
            "{carried}"
        );
        assert!(
            carried.ends_with("The user's next message follows.\n\n"),
            "{carried}"
        );

        let long = vec![
            json!({"role": "user", "text": "the oldest"}),
            json!({"role": "assistant", "text": "é".repeat(MAX_CARRIED)}),
            json!({"role": "user", "text": "the newest"}),
        ];
        let carried = carried_text("codex", &long);
        assert!(carried.contains("[earlier turns left out]"));
        assert!(!carried.contains("the oldest") && carried.contains("the newest"));
        assert!(carried.len() < MAX_CARRIED + 512, "{}", carried.len());
    }

    #[test]
    fn a_name_is_not_a_number() {
        for good in ["fix-tests", "a", "run_2", &"x".repeat(63)] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "12", "a b", "a/b", "ü", &"x".repeat(64)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
