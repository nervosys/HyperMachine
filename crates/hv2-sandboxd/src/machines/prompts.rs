//! Prompts: a coding agent run in a machine for the caller.
//!
//! A coding agent is a program: Claude Code, Codex and their like each have a
//! command-line form that takes one prompt, works, prints what it does as a
//! line of JSON at a time, and exits. Running one for a caller is therefore a
//! [run](super::runs) with a command line this module knows how to write and
//! an output it knows how to read:
//!
//! - `POST /machines/{name}/prompts` `{"provider", "prompt", "model",
//!   "resume", "unattended", "cwd", "user", "env"}` starts one.
//! - `GET  /machines/{name}/prompts` lists them.
//! - `GET  /machines/{name}/prompts/{id}?after=N` is one prompt, and the
//!   events its agent has printed from byte `N` of its output on.
//! - `POST /machines/{name}/prompts/{id}/interrupt` interrupts it.
//!
//! A prompt's ID is its run's, so everything a run offers is there too: its
//! standard error, its exit code, a signal.
//!
//! # What this does and does not provide
//!
//! It runs a harness that is already in the machine, with a key the caller
//! supplies in `env`. It installs neither. A machine made from a
//! distribution's image has `apt` and a network to install one with, and a
//! caller's `userData` is the place to do it.
//!
//! The prompt is not put into the command line. It reaches the harness in
//! the environment and the command line names the variable, so a prompt
//! with a quote or a `$(...)` in it is text and never shell.
//!
//! # Providers
//!
//! Each is a command line. The flags are the harness's own, and each line
//! was given to the harness as published (Claude Code 2.1.296, Codex CLI
//! 0.162.1) with no key: both took it, printed a first event naming a
//! session, and ended saying they had no key. Neither has been run with one
//! here, so what a harness does once it is talking to its model is its own.
//!
//! - `claude`: `claude -p` with `--output-format stream-json`.
//! - `codex`: `codex exec --json`.
//! - `custom`: the caller's own `command`, which reads `$HM_PROMPT`. For any
//!   other harness, and for trying this without one. An interrupt goes to
//!   the shell that runs the line, so a line that should be interrupted
//!   `exec`s its harness.
//!
//! # Not here
//!
//! Steering a turn that is running, and conversations as boat has them:
//! named, listed, moved between harnesses. `resume` carries a harness's own
//! session ID from one prompt to the next, where the harness has one.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex as StdMutex;

use axum::http::HeaderMap;
use serde_json::{json, Value};

use super::super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State, StatusCode,
};
use super::{named, runs};

/// The variable the prompt reaches the harness in.
const PROMPT_VAR: &str = "HM_PROMPT";
/// The model's name, when one is given.
const MODEL_VAR: &str = "HM_MODEL";
/// The harness's session to carry on from, when one is given.
const RESUME_VAR: &str = "HM_RESUME";

/// The most a prompt may be. It travels in the environment, which has a
/// limit of its own.
pub(crate) const MAX_PROMPT: usize = 128 << 10;

/// What is remembered of a prompt beside its run.
struct Prompt {
    provider: String,
    prompt: String,
    model: Option<String>,
    resume: Option<String>,
}

/// Every machine's prompts on this node: machine ID, then run ID.
static PROMPTS: StdMutex<Option<HashMap<String, BTreeMap<u64, Prompt>>>> = StdMutex::new(None);

fn prompts<T>(f: impl FnOnce(&mut HashMap<String, BTreeMap<u64, Prompt>>) -> T) -> T {
    let mut guard = PROMPTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Forget a machine's prompts, when it is deleted.
pub(crate) fn forget(machine_id: &str) {
    prompts(|all| all.remove(machine_id));
}

/// What a caller chose about how a harness is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choices {
    model: bool,
    resume: bool,
    /// Let the harness act without asking: there is nobody to ask.
    unattended: bool,
}

/// The command line that runs `provider` on the prompt in [`PROMPT_VAR`].
///
/// Every value a caller supplies is named by its variable and quoted, never
/// written into the line. A provider's harness is `exec`ed, so that it is the
/// run's own process and an interrupt reaches it and not a shell around it.
fn command_for(provider: &str, choices: Choices, custom: Option<&str>) -> Result<String, String> {
    let mut line = match provider {
        "claude" => {
            // `stream-json` in print mode needs `--verbose`; without it the
            // harness refuses to start.
            let mut line =
                format!("exec claude -p \"${PROMPT_VAR}\" --output-format stream-json --verbose");
            if choices.unattended {
                line.push_str(" --dangerously-skip-permissions");
            }
            if choices.resume {
                line.push_str(&format!(" --resume \"${RESUME_VAR}\""));
            }
            line
        }
        "codex" => {
            if choices.resume {
                return Err(
                    "resume is not offered for codex: its way of carrying a session on has \
                     not been checked here"
                        .to_string(),
                );
            }
            let mut line = "exec codex exec --json --skip-git-repo-check".to_string();
            if choices.unattended {
                line.push_str(" --dangerously-bypass-approvals-and-sandbox");
            }
            line.push_str(&format!(" \"${PROMPT_VAR}\""));
            line
        }
        "custom" => {
            let command = custom
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .ok_or("provider \"custom\" needs `command`: a shell line that reads $HM_PROMPT")?;
            return if choices.unattended || choices.resume {
                Err(
                    "`unattended` and `resume` are a provider's flags; a custom command \
                     says what it means itself"
                        .to_string(),
                )
            } else {
                Ok(command.to_string())
            };
        }
        other => {
            return Err(format!(
                "no provider {other:?}: claude, codex, or custom with a command"
            ))
        }
    };
    if custom.is_some() {
        return Err(format!(
            "`command` is for provider \"custom\"; {provider} has its own"
        ));
    }
    if choices.model {
        let flag = if provider == "codex" { "-m" } else { "--model" };
        // Before the prompt for codex, whose prompt is its last argument.
        let model = format!(" {flag} \"${MODEL_VAR}\"");
        match line.rfind(&format!(" \"${PROMPT_VAR}\"")) {
            Some(at) if provider == "codex" => line.insert_str(at, &model),
            _ => line.push_str(&model),
        }
    }
    Ok(line)
}

/// Why `provider` will not run as `user`, when it will not.
///
/// Claude Code refuses its unattended flag to root. It says so on its
/// standard error and exits, which a caller reading events sees as a prompt
/// that ended with none; said here, it is said where they are looking. A
/// run is root's unless a user is given.
fn refused_as_root(provider: &str, unattended: bool, user: Option<&str>) -> Option<&'static str> {
    let root = user.is_none_or(|user| matches!(user.trim(), "root" | "0"));
    (provider == "claude" && unattended && root).then_some(
        "claude will not run unattended as root: give `user`, a user of the guest that is not",
    )
}

/// The events in `text`, which starts at byte `base` of a harness's output.
///
/// An event is a line. One that is JSON is given as it is; one that is not,
/// a warning a harness printed before it started for one, is given as text
/// and not thrown away. Only whole lines count while the harness is running:
/// half a line is half an event. Returns the events and the byte to read
/// from next.
fn events(text: &str, base: u64, ended: bool) -> (Vec<Value>, u64) {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < text.len() {
        let rest = &text[at..];
        let (line, used) = match rest.find('\n') {
            Some(end) => (&rest[..end], end + 1),
            None if ended => (rest, rest.len()),
            None => break,
        };
        let trimmed = line.trim_end_matches('\r');
        if !trimmed.trim().is_empty() {
            let offset = base + at as u64;
            found.push(match serde_json::from_str::<Value>(trimmed) {
                Ok(event) if event.is_object() => json!({ "offset": offset, "event": event }),
                _ => json!({ "offset": offset, "text": trimmed }),
            });
        }
        at += used;
    }
    (found, base + at as u64)
}

/// The harness's own name for its session, where an event carries one: what
/// a later prompt gives as `resume`.
fn session_in(events: &[Value]) -> Option<String> {
    events.iter().find_map(|entry| {
        let event = entry.get("event")?;
        ["session_id", "thread_id", "conversation_id"]
            .iter()
            .find_map(|key| event.get(key)?.as_str().map(str::to_string))
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewPrompt {
    provider: String,
    prompt: String,
    model: Option<String>,
    /// A session of the harness's to carry on from: what an earlier prompt
    /// reported as its `session`.
    resume: Option<String>,
    /// Let the harness act without asking for approval. Off unless given:
    /// the flags that do it are the harness's "dangerous" ones, and a caller
    /// turns them on by saying so.
    #[serde(default)]
    unattended: bool,
    /// For provider `custom`: the command to run.
    command: Option<String>,
    cwd: Option<String>,
    user: Option<String>,
    /// The harness's key and anything else it reads from its environment.
    /// Not kept, and not returned.
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// A prompt as it is described: its run, and what was asked.
fn describe(run: Value, prompt: &Prompt) -> Value {
    let mut described = run;
    described["promptID"] = described["runID"].clone();
    described["provider"] = json!(prompt.provider);
    described["prompt"] = json!(prompt.prompt);
    described["model"] = json!(prompt.model);
    described["resume"] = json!(prompt.resume);
    // The command line names variables and holds nothing of the caller's; a
    // prompt is described by what was asked, not by how it was run.
    if let Some(object) = described.as_object_mut() {
        object.remove("cmd");
        object.remove("runID");
    }
    described
}

/// `POST /machines/{name}/prompts`.
pub(crate) async fn start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<NewPrompt>,
) -> Response {
    if req.prompt.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "prompt is empty");
    }
    if req.prompt.len() > MAX_PROMPT {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("prompt is at most {} KiB", MAX_PROMPT / 1024),
        );
    }
    for (what, value) in [("model", &req.model), ("resume", &req.resume)] {
        if value.as_ref().is_some_and(|v| v.trim().is_empty()) {
            return api_error(StatusCode::BAD_REQUEST, format!("{what} is empty"));
        }
    }
    let choices = Choices {
        model: req.model.is_some(),
        resume: req.resume.is_some(),
        unattended: req.unattended,
    };
    let cmd = match command_for(&req.provider, choices, req.command.as_deref()) {
        Ok(cmd) => cmd,
        Err(why) => return api_error(StatusCode::BAD_REQUEST, why),
    };
    if let Some(why) = refused_as_root(&req.provider, req.unattended, req.user.as_deref()) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    // The caller's environment first, then what this sets: a caller cannot
    // name the prompt's own variable and have the harness read something
    // else as its prompt.
    let mut env = req.env;
    env.insert(PROMPT_VAR.to_string(), req.prompt.clone());
    if let Some(model) = &req.model {
        env.insert(MODEL_VAR.to_string(), model.clone());
    }
    if let Some(resume) = &req.resume {
        env.insert(RESUME_VAR.to_string(), resume.clone());
    }
    let run = runs::NewRun {
        cmd,
        cwd: req.cwd,
        env,
        user: req.user,
        // Closed at once: a harness given a prompt as an argument must not
        // sit waiting for another on its standard input.
        stdin: Some(String::new()),
    };
    let (machine_id, id, described) = match runs::launch(&state, &headers, &name, run).await {
        Ok(started) => started,
        Err(response) => return response,
    };
    let prompt = Prompt {
        provider: req.provider,
        prompt: req.prompt,
        model: req.model,
        resume: req.resume,
    };
    let reply = describe(described, &prompt);
    prompts(|all| all.entry(machine_id).or_default().insert(id, prompt));
    (StatusCode::CREATED, Json(reply)).into_response()
}

/// `GET /machines/{name}/prompts`: every prompt kept for the machine, oldest
/// first, without events.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let listed: Vec<Value> = prompts(|all| {
        all.get(&machine.id)
            .map(|kept| {
                kept.iter()
                    // A prompt whose run made room for a newer one is gone
                    // with it.
                    .filter_map(|(id, prompt)| {
                        runs::with_run(&machine.id, *id, |run| describe(run.describe(), prompt))
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    Json(listed).into_response()
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EventsQuery {
    /// Read events from this byte of the harness's output on: the `next` of
    /// the last answer.
    after: Option<u64>,
}

/// `GET /machines/{name}/prompts/{id}`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, u64)>,
    axum::extract::Query(query): axum::extract::Query<EventsQuery>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let found = prompts(|all| {
        let prompt = all.get(&machine.id)?.get(&id)?;
        runs::with_run(&machine.id, id, |run| {
            let (offset, text, _) = run.stdout_from(query.after.unwrap_or(0));
            let (found, next) = events(text, offset, !run.is_running());
            let mut reply = describe(run.describe(), prompt);
            reply["session"] = json!(session_in(&found));
            reply["events"] = json!(found);
            reply["next"] = json!(next);
            reply
        })
    });
    match found {
        Some(reply) => Json(reply).into_response(),
        None => api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no prompt {id}"),
        ),
    }
}

/// `POST /machines/{name}/prompts/{id}/interrupt`: what Ctrl-C would be.
pub(crate) async fn interrupt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, id)): Path<(String, u64)>,
) -> Response {
    let machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let known = prompts(|all| {
        all.get(&machine.id)
            .is_some_and(|kept| kept.contains_key(&id))
    });
    if !known {
        return api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no prompt {id}"),
        );
    }
    const SIGINT: i32 = 2;
    match runs::signal_run(&state, &headers, &name, id, SIGINT).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Choices = Choices {
        model: false,
        resume: false,
        unattended: false,
    };

    #[test]
    fn each_provider_s_command_names_its_variables_and_holds_no_value() {
        assert_eq!(
            command_for("claude", PLAIN, None).unwrap(),
            r#"exec claude -p "$HM_PROMPT" --output-format stream-json --verbose"#
        );
        let all = Choices {
            model: true,
            resume: true,
            unattended: true,
        };
        assert_eq!(
            command_for("claude", all, None).unwrap(),
            r#"exec claude -p "$HM_PROMPT" --output-format stream-json --verbose --dangerously-skip-permissions --resume "$HM_RESUME" --model "$HM_MODEL""#
        );
        assert_eq!(
            command_for("codex", PLAIN, None).unwrap(),
            r#"exec codex exec --json --skip-git-repo-check "$HM_PROMPT""#
        );
        // The model goes before the prompt, which is codex's last argument.
        let model = Choices {
            model: true,
            unattended: true,
            ..PLAIN
        };
        assert_eq!(
            command_for("codex", model, None).unwrap(),
            r#"exec codex exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox -m "$HM_MODEL" "$HM_PROMPT""#
        );
        assert_eq!(
            command_for(
                "custom",
                PLAIN,
                Some("  my-agent --prompt \"$HM_PROMPT\"  ")
            )
            .unwrap(),
            r#"my-agent --prompt "$HM_PROMPT""#
        );
    }

    #[test]
    fn claude_unattended_needs_a_user_that_is_not_root() {
        for user in [None, Some("root"), Some("0"), Some(" root ")] {
            assert!(refused_as_root("claude", true, user).is_some(), "{user:?}");
            // Attended, or another harness: theirs to say.
            assert_eq!(refused_as_root("claude", false, user), None);
            assert_eq!(refused_as_root("codex", true, user), None);
        }
        assert_eq!(refused_as_root("claude", true, Some("dev")), None);
        assert_eq!(refused_as_root("claude", true, Some("1000")), None);
    }

    #[test]
    fn what_a_provider_cannot_do_is_refused_and_not_dropped() {
        let resume = Choices {
            resume: true,
            ..PLAIN
        };
        let unattended = Choices {
            unattended: true,
            ..PLAIN
        };
        for (provider, choices, custom, expect) in [
            ("codex", resume, None, "not offered for codex"),
            ("custom", PLAIN, None, "needs `command`"),
            ("custom", PLAIN, Some("   "), "needs `command`"),
            ("custom", unattended, Some("x"), "a provider's flags"),
            ("custom", resume, Some("x"), "a provider's flags"),
            ("claude", PLAIN, Some("x"), "is for provider"),
            ("gpt", PLAIN, None, "no provider"),
            ("", PLAIN, None, "no provider"),
        ] {
            let refused = command_for(provider, choices, custom).unwrap_err();
            assert!(refused.contains(expect), "{provider}: {refused}");
        }
    }

    #[test]
    fn events_are_whole_lines_json_or_not() {
        let text =
            "warning: something\n{\"type\":\"system\",\"session_id\":\"abc\"}\n\n{\"type\":\"res";
        // Running: the last line is not whole yet, and is left for the next
        // read, which starts where it does.
        let (found, next) = events(text, 100, false);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0],
            json!({"offset": 100, "text": "warning: something"})
        );
        assert_eq!(found[1]["offset"], 119);
        assert_eq!(found[1]["event"]["session_id"], "abc");
        let start_of_partial = 100 + text.rfind("{\"type\":\"res").unwrap() as u64;
        assert_eq!(next, start_of_partial);
        assert_eq!(session_in(&found).as_deref(), Some("abc"));

        // Ended: what is left is all there will be, so it counts.
        let (found, next) = events(text, 100, true);
        assert_eq!(found.len(), 3);
        assert_eq!(found[2]["text"], "{\"type\":\"res");
        assert_eq!(next, 100 + text.len() as u64);

        // The rest of that line arriving completes it into an event, read
        // from where the last read stopped.
        let more = "{\"type\":\"result\",\"ok\":true}\r\n";
        let (found, next) = events(more, start_of_partial, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["offset"], start_of_partial);
        assert_eq!(found[0]["event"]["ok"], true);
        assert_eq!(next, start_of_partial + more.len() as u64);

        // A line of JSON that is not an object is text: an event has fields.
        let (found, _) = events("42\n\"quoted\"\n", 0, true);
        assert_eq!(found[0], json!({"offset": 0, "text": "42"}));
        assert_eq!(found[1]["text"], "\"quoted\"");
        assert_eq!(events("", 7, true), (Vec::new(), 7));
        assert_eq!(session_in(&[]), None);
    }

    #[test]
    fn a_prompt_is_described_by_what_was_asked() {
        let run = json!({
            "runID": 4, "cmd": "claude -p \"$HM_PROMPT\"", "pid": 10, "state": "running",
            "exitCode": null, "signal": null, "reason": null, "startedAt": 1, "endedAt": null,
            "stdoutBytes": 0, "stderrBytes": 0,
        });
        let prompt = Prompt {
            provider: "claude".into(),
            prompt: "fix the tests".into(),
            model: Some("opus".into()),
            resume: None,
        };
        let described = describe(run, &prompt);
        assert_eq!(described["promptID"], 4);
        assert_eq!(described["provider"], "claude");
        assert_eq!(described["prompt"], "fix the tests");
        assert_eq!(described["model"], "opus");
        assert!(described["resume"].is_null());
        assert_eq!(described["state"], "running");
        assert!(described.get("cmd").is_none() && described.get("runID").is_none());
        assert!(described.get("env").is_none());
    }
}
