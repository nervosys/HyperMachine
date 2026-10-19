//! A Dockerfile, turned into the steps the node's template build runs.
//!
//! This is a parser for what a build here can do, not for all of Docker's
//! language. It reads `FROM`, `RUN`, `ENV`, `ARG`, `WORKDIR`, `USER`, `COPY`,
//! `ADD` (of local files), `CMD` and `ENTRYPOINT`; it ignores `LABEL`,
//! `EXPOSE`, `MAINTAINER`, `STOPSIGNAL`, `HEALTHCHECK` and `VOLUME` with a
//! warning; and it refuses, by name, what it cannot do: a second `FROM`
//! (multi-stage builds), `COPY --from`, `ADD` of a URL, here-documents,
//! `SHELL`, `ONBUILD` and flags such as `RUN --mount`.
//!
//! `ARG`s and `ENV`s are substituted into the instructions Docker substitutes
//! them into (`$NAME`, `${NAME}`, `${NAME:-default}`, `${NAME:+word}`); an
//! `ARG` is also exported to the `RUN`s that follow it, as Docker does.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

/// One step of the node's build API: `{type, args, filesHash?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: &'static str,
    pub args: Vec<String>,
    /// A `COPY`: its source is in the build context's archive.
    pub from_context: bool,
}

impl Step {
    pub fn to_json(&self, files_hash: Option<&str>) -> Value {
        let mut step = json!({"type": self.kind, "args": self.args});
        if let (true, Some(hash)) = (self.from_context, files_hash) {
            step["filesHash"] = json!(hash);
        }
        step
    }
}

/// What a Dockerfile asks for.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub image: String,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn copies_from_context(&self) -> bool {
        self.steps.iter().any(|s| s.from_context)
    }
}

/// Parse `text`, with `build_args` overriding `ARG` defaults.
pub fn parse(text: &str, build_args: &BTreeMap<String, String>) -> Result<Plan> {
    let mut plan = Plan::default();
    // Variables an instruction may name: ARGs (global ones before FROM too),
    // then ENVs, which win.
    let mut args: BTreeMap<String, String> = BTreeMap::new();
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut declared: Vec<String> = Vec::new();
    let mut seen_from = false;

    for (line_number, logical) in logical_lines(text)? {
        let at = |message: String| anyhow!("Dockerfile line {line_number}: {message}");
        let (word, rest) = match logical.split_once(char::is_whitespace) {
            Some((word, rest)) => (word, rest.trim()),
            None => (logical.as_str(), ""),
        };
        let keyword = word.to_ascii_uppercase();
        if rest.contains("<<") && matches!(keyword.as_str(), "RUN" | "COPY" | "ADD") {
            let heredoc = rest
                .split_whitespace()
                .any(|w| w.starts_with("<<") && w.len() > 2);
            if heredoc {
                return Err(at("here-documents are not supported".into()));
            }
        }
        match keyword.as_str() {
            "ARG" => {
                let items = words(rest, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?;
                for item in items {
                    let (name, default) = match item.split_once('=') {
                        Some((n, d)) => (n.to_string(), Some(d.to_string())),
                        None => (item.clone(), None),
                    };
                    if let Some(value) = build_args.get(&name).cloned().or(default) {
                        args.insert(name.clone(), value);
                    }
                    if seen_from {
                        declared.push(name);
                    }
                }
            }
            "FROM" => {
                if seen_from {
                    return Err(at(
                        "multi-stage builds are not supported (a second FROM)".into()
                    ));
                }
                let mut parts = words(rest, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?;
                parts.retain(|p| !p.starts_with("--platform="));
                let image = parts
                    .first()
                    .ok_or_else(|| at("FROM without an image".into()))?;
                if image.eq_ignore_ascii_case("scratch") {
                    return Err(at("FROM scratch is not supported".into()));
                }
                plan.image = image.clone();
                seen_from = true;
                // ARGs declared before FROM are not visible after it.
                args.clear();
            }
            _ if !seen_from => {
                return Err(at(format!("{word} before FROM")));
            }
            "RUN" => {
                if rest.starts_with("--") {
                    return Err(at(format!(
                        "RUN flags ({}) are not supported",
                        rest.split_whitespace().next().unwrap_or("")
                    )));
                }
                let mut command = match exec_form(rest) {
                    Some(argv) => shell_join(&argv),
                    None => rest.to_string(),
                };
                // Docker passes the ARGs seen so far to a RUN as environment.
                let exports: String = declared
                    .iter()
                    .filter_map(|n| args.get(n).map(|v| format!("export {n}={}; ", quote(v))))
                    .collect();
                command.insert_str(0, &exports);
                plan.steps.push(step("RUN", vec![command]));
            }
            "ENV" => {
                let pairs = env_pairs(rest, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?;
                if pairs.is_empty() {
                    return Err(at("ENV without a variable".into()));
                }
                let mut flat = Vec::new();
                for (name, value) in pairs {
                    env.insert(name.clone(), value.clone());
                    flat.push(name);
                    flat.push(value);
                }
                plan.steps.push(step("ENV", flat));
            }
            "WORKDIR" => {
                let dir = substitute(rest, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?;
                if dir.is_empty() {
                    return Err(at("WORKDIR without a path".into()));
                }
                plan.steps.push(step("WORKDIR", vec![dir]));
            }
            "USER" => {
                let user = substitute(rest, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?;
                let (name, group) = user.split_once(':').unwrap_or((user.as_str(), ""));
                if name.is_empty() {
                    return Err(at("USER without a user".into()));
                }
                if !group.is_empty() {
                    plan.warnings.push(format!(
                        "line {line_number}: USER {user}: the group is ignored"
                    ));
                }
                plan.steps.push(step("USER", vec![name.to_string()]));
            }
            "COPY" | "ADD" => {
                let (flags, operands) = split_flags(rest);
                let mut owner = String::new();
                let mut mode = String::new();
                for flag in flags {
                    match flag.split_once('=') {
                        Some(("--chown", v)) => owner = v.to_string(),
                        Some(("--chmod", v)) => mode = v.to_string(),
                        Some(("--from", _)) => {
                            return Err(at("COPY --from (multi-stage) is not supported".into()))
                        }
                        _ if flag.starts_with("--link") => {}
                        _ => return Err(at(format!("{keyword} flag {flag} is not supported"))),
                    }
                }
                let mut paths = match exec_form(&operands) {
                    Some(argv) => argv,
                    None => {
                        words(&operands, &lookup(&args, &env)).map_err(|e| at(e.to_string()))?
                    }
                };
                if paths.len() < 2 {
                    return Err(at(format!("{keyword} needs a source and a destination")));
                }
                let dest = paths.pop().expect("two or more");
                if paths.len() > 1 && !dest.ends_with('/') {
                    return Err(at(format!(
                        "{keyword} of several sources needs a destination ending in /"
                    )));
                }
                for source in paths {
                    if keyword == "ADD" && (source.contains("://") || is_archive(&source)) {
                        return Err(at(format!(
                            "ADD of {source}: only plain local files are supported; use COPY, or RUN curl / tar"
                        )));
                    }
                    let mut copy = step(
                        "COPY",
                        vec![source, dest.clone(), owner.clone(), mode.clone()],
                    );
                    copy.from_context = true;
                    plan.steps.push(copy);
                }
            }
            "CMD" | "ENTRYPOINT" => {
                let argv = match exec_form(rest) {
                    Some(argv) => argv,
                    None => vec!["/bin/sh".into(), "-c".into(), rest.to_string()],
                };
                plan.steps.push(step(
                    if keyword == "CMD" {
                        "CMD"
                    } else {
                        "ENTRYPOINT"
                    },
                    argv,
                ));
            }
            "LABEL" | "EXPOSE" | "MAINTAINER" | "STOPSIGNAL" | "HEALTHCHECK" | "VOLUME" => {
                plan.warnings
                    .push(format!("line {line_number}: {keyword} is ignored"));
            }
            "SHELL" | "ONBUILD" => return Err(at(format!("{keyword} is not supported"))),
            _ => return Err(at(format!("unknown instruction {word}"))),
        }
    }
    if !seen_from {
        bail!("Dockerfile has no FROM");
    }
    Ok(plan)
}

fn step(kind: &'static str, args: Vec<String>) -> Step {
    Step {
        kind,
        args,
        from_context: false,
    }
}

fn is_archive(source: &str) -> bool {
    [".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz", ".zip"]
        .iter()
        .any(|ext| source.ends_with(ext))
}

/// The instructions, each with the number of the line it began on:
/// comments and blank lines dropped, backslash continuations joined.
fn logical_lines(text: &str) -> Result<Vec<(usize, String)>> {
    let mut lines = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (index, raw) in text.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        let trimmed = raw.trim();
        // A comment line, or a blank one, inside a continuation is skipped.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (number, mut text) = current.take().unwrap_or((index + 1, String::new()));
        let body = raw.trim_start();
        if let Some(stripped) = body.strip_suffix('\\') {
            text.push_str(stripped);
            current = Some((number, text));
        } else {
            text.push_str(body);
            lines.push((number, text.trim().to_string()));
        }
    }
    if let Some((number, text)) = current {
        lines.push((number, text.trim().to_string()));
    }
    Ok(lines)
}

/// `["a", "b"]` as its elements; `None` if `text` is not a JSON string array.
fn exec_form(text: &str) -> Option<Vec<String>> {
    let text = text.trim();
    if !text.starts_with('[') {
        return None;
    }
    serde_json::from_str::<Vec<String>>(text).ok()
}

/// Flags at the front of `text` (`--chown=...`), and the rest.
fn split_flags(text: &str) -> (Vec<String>, String) {
    let mut flags = Vec::new();
    let mut rest = text.trim_start();
    while rest.starts_with("--") {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        flags.push(rest[..end].to_string());
        rest = rest[end..].trim_start();
    }
    (flags, rest.to_string())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn shell_join(argv: &[String]) -> String {
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

type Lookup<'a> = Box<dyn Fn(&str) -> Option<String> + 'a>;

/// ENV over ARG, as in Docker.
fn lookup<'a>(args: &'a BTreeMap<String, String>, env: &'a BTreeMap<String, String>) -> Lookup<'a> {
    Box::new(move |name| env.get(name).or_else(|| args.get(name)).cloned())
}

/// `text` with `$NAME`, `${NAME}`, `${NAME:-d}` and `${NAME:+w}` replaced.
fn substitute(text: &str, vars: &Lookup<'_>) -> Result<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_single = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_single = !in_single;
                out.push(c);
                i += 1;
            }
            '\\' if !in_single && matches!(chars.get(i + 1), Some('$') | Some('\\')) => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '$' if !in_single => {
                if chars.get(i + 1) == Some(&'{') {
                    let close = chars[i..]
                        .iter()
                        .position(|&c| c == '}')
                        .ok_or_else(|| anyhow!("unterminated ${{ in {text:?}"))?;
                    let inner: String = chars[i + 2..i + close].iter().collect();
                    let (name, op, word) = match inner.find(":-").or_else(|| inner.find(":+")) {
                        Some(at) => (&inner[..at], Some(&inner[at..at + 2]), &inner[at + 2..]),
                        None => (inner.as_str(), None, ""),
                    };
                    let value = vars(name).filter(|v| !v.is_empty());
                    match (op, value) {
                        (Some(":-"), None) => out.push_str(word),
                        (Some(":+"), Some(_)) => out.push_str(word),
                        (Some(":+"), None) => {}
                        (_, Some(v)) => out.push_str(&v),
                        (_, None) => {}
                    }
                    i += close + 1;
                } else {
                    let name: String = chars[i + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                        .collect();
                    if name.is_empty() {
                        out.push('$');
                        i += 1;
                    } else {
                        if let Some(v) = vars(&name) {
                            out.push_str(&v);
                        }
                        i += 1 + name.len();
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Whitespace-separated words, with quotes removed and variables substituted.
fn words(text: &str, vars: &Lookup<'_>) -> Result<Vec<String>> {
    let text = substitute(text, vars)?;
    split_words(&text)
}

fn split_words(text: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.peek() {
                Some(&n) if matches!(n, '"' | '\\' | '$') => {
                    word.push(n);
                    chars.next();
                }
                _ => word.push('\\'),
            },
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        bail!("an unterminated quote in {text:?}");
    }
    if started {
        out.push(word);
    }
    Ok(out)
}

/// `ENV a=1 b="two words"` or the old `ENV a one two words`.
fn env_pairs(rest: &str, vars: &Lookup<'_>) -> Result<Vec<(String, String)>> {
    let first = rest.split_whitespace().next().unwrap_or("");
    if first.contains('=') {
        return words(rest, vars)?
            .into_iter()
            .map(|pair| {
                pair.split_once('=')
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .ok_or_else(|| anyhow!("ENV {pair}: expected NAME=VALUE"))
            })
            .collect();
    }
    match rest.split_once(char::is_whitespace) {
        Some((name, value)) => Ok(vec![(name.to_string(), substitute(value.trim(), vars)?)]),
        None if rest.is_empty() => Ok(Vec::new()),
        None => bail!("ENV {rest}: expected NAME=VALUE"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(text: &str) -> Plan {
        parse(text, &BTreeMap::new()).unwrap()
    }

    fn error(text: &str) -> String {
        parse(text, &BTreeMap::new()).unwrap_err().to_string()
    }

    fn kinds(plan: &Plan) -> Vec<(&'static str, Vec<String>)> {
        plan.steps
            .iter()
            .map(|s| (s.kind, s.args.clone()))
            .collect()
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_common_instructions_become_steps() {
        let p = plan(
            "# a comment\nFROM python:3.12-slim AS app\n\nWORKDIR /srv\nENV A=1 B=\"two words\"\n\
             RUN pip install \\\n    requests\nUSER nobody\nCOPY --chown=nobody:nogroup app.py /srv/\nCMD [\"python\", \"app.py\"]\n",
        );
        assert_eq!(p.image, "python:3.12-slim");
        assert_eq!(
            kinds(&p),
            vec![
                ("WORKDIR", v(&["/srv"])),
                ("ENV", v(&["A", "1", "B", "two words"])),
                ("RUN", v(&["pip install requests"])),
                ("USER", v(&["nobody"])),
                ("COPY", v(&["app.py", "/srv/", "nobody:nogroup", ""])),
                ("CMD", v(&["python", "app.py"])),
            ]
        );
        assert!(p.copies_from_context());
    }

    #[test]
    fn shell_form_cmd_and_entrypoint_run_through_sh() {
        let p = plan("FROM alpine\nENTRYPOINT exec nginx -g 'daemon off;'\nCMD echo hi\n");
        assert_eq!(
            kinds(&p),
            vec![
                (
                    "ENTRYPOINT",
                    v(&["/bin/sh", "-c", "exec nginx -g 'daemon off;'"])
                ),
                ("CMD", v(&["/bin/sh", "-c", "echo hi"])),
            ]
        );
    }

    #[test]
    fn args_and_envs_are_substituted_where_docker_does() {
        let mut given = BTreeMap::new();
        given.insert("VERSION".to_string(), "3.20".to_string());
        let p = parse(
            "ARG VERSION=latest\nFROM alpine:${VERSION}\nARG MODE=dev\nENV HOME_DIR=/srv/$MODE\nWORKDIR ${HOME_DIR}/app\nRUN echo $MODE\nENV X=${NOPE:-fallback} Y=${MODE:+set}\n",
            &given,
        )
        .unwrap();
        assert_eq!(p.image, "alpine:3.20");
        assert_eq!(
            kinds(&p),
            vec![
                ("ENV", v(&["HOME_DIR", "/srv/dev"])),
                ("WORKDIR", v(&["/srv/dev/app"])),
                ("RUN", v(&["export MODE='dev'; echo $MODE"])),
                ("ENV", v(&["X", "fallback", "Y", "set"])),
            ]
        );
    }

    #[test]
    fn a_build_arg_overrides_a_default_and_an_arg_before_from_is_not_visible_after() {
        let mut given = BTreeMap::new();
        given.insert("TAG".to_string(), "9".to_string());
        let p = parse("ARG TAG=1\nFROM img:$TAG\nWORKDIR /x$TAG\n", &given).unwrap();
        assert_eq!(p.image, "img:9");
        assert_eq!(kinds(&p), vec![("WORKDIR", v(&["/x"]))]);
    }

    #[test]
    fn the_old_env_form_takes_the_rest_of_the_line() {
        let p = plan("FROM a\nENV GREETING hello big world\n");
        assert_eq!(
            kinds(&p),
            vec![("ENV", v(&["GREETING", "hello big world"]))]
        );
    }

    #[test]
    fn exec_form_run_is_quoted_for_the_shell() {
        let p = plan("FROM a\nRUN [\"echo\", \"it's\", \"a b\"]\n");
        assert_eq!(kinds(&p), vec![("RUN", v(&["'echo' 'it'\\''s' 'a b'"]))]);
    }

    #[test]
    fn several_sources_make_a_copy_each() {
        let p = plan("FROM a\nCOPY a.txt b.txt /dst/\n");
        assert_eq!(
            kinds(&p),
            vec![
                ("COPY", v(&["a.txt", "/dst/", "", ""])),
                ("COPY", v(&["b.txt", "/dst/", "", ""])),
            ]
        );
        assert!(error("FROM a\nCOPY a b /dst\n").contains("ending in /"));
    }

    #[test]
    fn what_cannot_be_done_is_refused_by_name() {
        for (text, expected) in [
            ("FROM a\nFROM b\n", "multi-stage"),
            ("FROM a\nCOPY --from=build /x /y\n", "multi-stage"),
            ("FROM scratch\n", "scratch"),
            (
                "FROM a\nRUN --mount=type=cache,target=/c make\n",
                "RUN flags",
            ),
            ("FROM a\nADD https://x.example/f /f\n", "ADD of"),
            ("FROM a\nADD app.tar.gz /srv/\n", "ADD of"),
            ("FROM a\nRUN cat <<EOF\nx\nEOF\n", "here-documents"),
            ("FROM a\nSHELL [\"/bin/bash\", \"-c\"]\n", "SHELL"),
            ("FROM a\nONBUILD RUN x\n", "ONBUILD"),
            ("RUN echo hi\n", "before FROM"),
            ("FROM a\nFROBNICATE x\n", "unknown instruction"),
            ("# nothing\n", "no FROM"),
        ] {
            let message = error(text);
            assert!(message.contains(expected), "{text:?}: {message}");
        }
    }

    #[test]
    fn errors_name_the_line_an_instruction_began_on() {
        assert!(error("FROM a\n\nRUN ok\nFROBNICATE x\n").starts_with("Dockerfile line 4:"));
    }

    #[test]
    fn ignored_instructions_warn_and_a_group_in_user_is_dropped() {
        let p = plan("FROM a\nEXPOSE 80\nLABEL a=b\nUSER app:grp\n");
        assert_eq!(kinds(&p), vec![("USER", v(&["app"]))]);
        assert_eq!(p.warnings.len(), 3, "{:?}", p.warnings);
    }

    #[test]
    fn crlf_line_endings_and_comments_inside_a_continuation_are_handled() {
        let p = plan("FROM a\r\nRUN one \\\r\n# not part\r\n  two\r\n");
        assert_eq!(kinds(&p), vec![("RUN", v(&["one two"]))]);
    }

    #[test]
    fn a_step_names_its_files_hash_only_for_a_copy() {
        let p = plan("FROM a\nRUN x\nCOPY f /f\n");
        assert_eq!(
            p.steps[0].to_json(Some("h")),
            json!({"type": "RUN", "args": ["x"]})
        );
        assert_eq!(
            p.steps[1].to_json(Some("h")),
            json!({"type": "COPY", "args": ["f", "/f", "", ""], "filesHash": "h"})
        );
    }
}
