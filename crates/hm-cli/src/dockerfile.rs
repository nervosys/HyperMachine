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
    /// A `COPY --from=NAME`: its source is in an earlier stage, named as the
    /// Dockerfile wrote it (a stage's name or its number).
    pub from_stage: Option<String>,
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
    /// `FROM image AS name`.
    pub name: Option<String>,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn copies_from_context(&self) -> bool {
        self.steps.iter().any(|s| s.from_context)
    }
}

/// A Dockerfile of several stages: every stage's plan, in order. The last
/// stage is the image; the others are only read by `COPY --from`.
pub fn parse_stages(text: &str, build_args: &BTreeMap<String, String>) -> Result<Vec<Plan>> {
    // The instructions before the first FROM (global ARGs) are put in front
    // of every stage, where `parse` already knows what to do with them.
    let mut preamble = String::new();
    let mut stages: Vec<String> = Vec::new();
    let mut continued = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let starts = !continued
            && trimmed
                .split_whitespace()
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("FROM"));
        if starts {
            stages.push(String::new());
        }
        let target = match stages.last_mut() {
            Some(stage) => stage,
            None => &mut preamble,
        };
        target.push_str(raw);
        target.push('\n');
        let body = raw.trim_end_matches('\r').trim_end();
        let is_comment = trimmed.starts_with('#') || trimmed.is_empty();
        if !is_comment {
            continued = body.ends_with('\\');
        }
    }
    if stages.is_empty() {
        bail!("Dockerfile has no FROM");
    }
    let mut plans: Vec<Plan> = Vec::new();
    let mut start = preamble.lines().count();
    for stage in &stages {
        // Blank lines keep each stage's line numbers those of the whole file.
        let blanks = "\n".repeat(start - preamble.lines().count());
        let padded = format!("{preamble}{blanks}{stage}");
        start += stage.lines().count();
        let plan = parse(&padded, build_args)?;
        plans.push(plan);
    }
    Ok(plans)
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
    // The SHELL instruction's argv; the default is `/bin/sh -c`.
    let mut shell: Option<Vec<String>> = None;

    for (line_number, logical) in logical_lines(text)? {
        let at = |message: String| anyhow!("Dockerfile line {line_number}: {message}");
        let (word, rest) = match logical.split_once(char::is_whitespace) {
            Some((word, rest)) => (word, rest.trim()),
            None => (logical.as_str(), ""),
        };
        let keyword = word.to_ascii_uppercase();
        if rest.contains("<<") && matches!(keyword.as_str(), "COPY" | "ADD") {
            let first_line = rest.lines().next().unwrap_or("");
            let heredoc = first_line
                .split_whitespace()
                .any(|w| w.starts_with("<<") && w.len() > 2);
            if heredoc {
                if keyword == "ADD" || !seen_from {
                    return Err(at(format!(
                        "{keyword} with a here-document is not supported"
                    )));
                }
                copy_heredoc(rest, &lookup(&args, &env), &mut plan)
                    .map_err(|e| at(e.to_string()))?;
                continue;
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
                        "a second FROM in one stage (use parse_stages for several stages)".into(),
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
                if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("as") {
                    plan.name = Some(parts[2].clone());
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
                let (flags, rest) = split_flags(rest);
                let rest = rest.as_str();
                let mut mounts: Vec<Mount> = Vec::new();
                for flag in &flags {
                    match flag.strip_prefix("--mount=") {
                        Some(spec) => {
                            let mount = parse_mount(spec).map_err(|e| at(e.to_string()))?;
                            if mount.kind == "cache" {
                                plan.warnings.push(format!(
                                    "line {line_number}: RUN --mount=type=cache: the cache is not \
                                     kept between builds (an empty directory for this RUN)"
                                ));
                            }
                            mounts.push(mount);
                        }
                        None => {
                            return Err(at(format!("RUN flag {flag} is not supported")));
                        }
                    }
                }
                let mut command = match exec_form(rest) {
                    Some(argv) => shell_join(&argv),
                    // `RUN <<EOF` runs the body as a script.
                    None if rest.starts_with("<<") => format!("/bin/sh {rest}"),
                    None => match &shell {
                        Some(shell) => {
                            shell_join(&[shell.clone(), vec![rest.to_string()]].concat())
                        }
                        None => rest.to_string(),
                    },
                };
                // Docker passes the ARGs seen so far to a RUN as environment.
                let exports: String = declared
                    .iter()
                    .filter_map(|n| args.get(n).map(|v| format!("export {n}={}; ", quote(v))))
                    .collect();
                command.insert_str(0, &exports);
                // Mounts last only for this RUN: a bind mount is copied in
                // first, and every target is removed when the command ends.
                for mount in &mounts {
                    if mount.kind == "bind" {
                        let mut copy = step(
                            "COPY",
                            vec![
                                mount.source.clone(),
                                mount.target.clone(),
                                String::new(),
                                String::new(),
                            ],
                        );
                        copy.from_context = mount.from.is_none();
                        copy.from_stage = mount.from.clone();
                        plan.steps.push(copy);
                    }
                }
                if !mounts.is_empty() {
                    let mut before = String::new();
                    let mut after = String::new();
                    for (n, mount) in mounts.iter().enumerate() {
                        let target = quote(&mount.target);
                        if mount.kind == "bind" {
                            after.push_str(&format!("rm -rf {target}; "));
                        } else {
                            before.push_str(&format!(
                                "made{n}=; [ -d {target} ] || {{ mkdir -p {target} && made{n}=1; }}; "
                            ));
                            after.push_str(&format!("[ -z \"$made{n}\" ] || rm -rf {target}; "));
                        }
                    }
                    command = format!("{before}{{\n{command}\n}}; status=$?; {after}exit $status");
                }
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
                let mut from_stage: Option<String> = None;
                for flag in flags {
                    match flag.split_once('=') {
                        Some(("--chown", v)) => owner = v.to_string(),
                        Some(("--chmod", v)) => mode = v.to_string(),
                        Some(("--from", v)) if keyword == "COPY" && !v.is_empty() => {
                            from_stage = Some(v.to_string());
                        }
                        Some(("--from", _)) => {
                            return Err(at(format!("{keyword} --from needs a COPY and a stage")))
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
                    if keyword == "ADD" && source.contains("://") {
                        return Err(at(format!(
                            "ADD of {source}: only local files are supported; use COPY, or RUN curl"
                        )));
                    }
                    if keyword == "ADD" && is_archive(&source) {
                        // Docker unpacks a local archive into the destination
                        // directory: copy it aside, unpack, remove it.
                        let aside = format!("/tmp/.hm-add-{}.tar", plan.steps.len());
                        let mut copy = step(
                            "COPY",
                            vec![source, aside.clone(), owner.clone(), mode.clone()],
                        );
                        copy.from_context = true;
                        plan.steps.push(copy);
                        let into = quote(&dest);
                        plan.steps.push(step(
                            "RUN",
                            vec![format!(
                                "mkdir -p {into} && {{ tar -xf {aside} -C {into} 2>/dev/null \
                                 || /bin/busybox tar -xf {aside} -C {into}; }}; \
                                 status=$?; rm -f {aside}; exit $status"
                            )],
                        ));
                        continue;
                    }
                    let mut copy = step(
                        "COPY",
                        vec![source, dest.clone(), owner.clone(), mode.clone()],
                    );
                    copy.from_context = from_stage.is_none();
                    copy.from_stage = from_stage.clone();
                    plan.steps.push(copy);
                }
            }
            "CMD" | "ENTRYPOINT" => {
                let argv = match exec_form(rest) {
                    Some(argv) => argv,
                    None => [
                        shell
                            .clone()
                            .unwrap_or_else(|| vec!["/bin/sh".into(), "-c".into()]),
                        vec![rest.to_string()],
                    ]
                    .concat(),
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
            "SHELL" => match exec_form(rest) {
                Some(argv) if !argv.is_empty() => shell = Some(argv),
                _ => {
                    return Err(at(
                        "SHELL needs a JSON array, like [\"/bin/sh\", \"-ec\"]".into()
                    ))
                }
            },
            "ONBUILD" => return Err(at(format!("{keyword} is not supported"))),
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
        from_stage: None,
    }
}

fn is_archive(source: &str) -> bool {
    [".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz"]
        .iter()
        .any(|ext| source.ends_with(ext))
}

/// The instructions, each with the number of the line it began on:
/// comments and blank lines dropped, backslash continuations joined.
fn logical_lines(text: &str) -> Result<Vec<(usize, String)>> {
    let mut lines = Vec::new();
    let mut current: Option<(usize, String)> = None;
    let mut source = text.lines().enumerate();
    while let Some((index, raw)) = source.next() {
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
            let mut text = text.trim().to_string();
            // `RUN <<EOF`: the body and its terminator belong to this
            // instruction, verbatim (the shell reads the here-document).
            if text
                .split_whitespace()
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("RUN") || w.eq_ignore_ascii_case("COPY"))
            {
                for (marker, strip_tabs) in heredoc_markers(&text) {
                    let mut closed = false;
                    for (_, line) in source.by_ref() {
                        let line = line.trim_end_matches('\r');
                        text.push('\n');
                        text.push_str(line);
                        let compare = if strip_tabs {
                            line.trim_start_matches('\t')
                        } else {
                            line
                        };
                        if compare == marker {
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        bail!("Dockerfile line {number}: here-document {marker} is never closed");
                    }
                }
            }
            lines.push((number, text));
        }
    }
    if let Some((number, text)) = current {
        lines.push((number, text.trim().to_string()));
    }
    Ok(lines)
}

/// One `RUN --mount=...` after parsing.
struct Mount {
    kind: String,
    source: String,
    target: String,
    from: Option<String>,
}

/// `type=bind,source=src,target=/t[,from=stage][,rw]`; bind, tmpfs and cache
/// are honoured, secret and ssh are refused (they would land in a layer).
fn parse_mount(spec: &str) -> Result<Mount> {
    let mut mount = Mount {
        kind: "bind".into(),
        source: ".".into(),
        target: String::new(),
        from: None,
    };
    for option in spec.split(',').filter(|o| !o.is_empty()) {
        let (key, value) = option.split_once('=').unwrap_or((option, ""));
        match key {
            "type" => mount.kind = value.to_string(),
            "source" | "src" => {
                mount.source = value.trim_start_matches('/').to_string();
                if mount.source.is_empty() {
                    mount.source = ".".into();
                }
            }
            "target" | "dst" | "destination" => mount.target = value.to_string(),
            "from" => mount.from = Some(value.to_string()),
            _ => {}
        }
    }
    if !matches!(mount.kind.as_str(), "bind" | "tmpfs" | "cache") {
        bail!(
            "RUN --mount=type={} is not supported (bind, tmpfs and cache are)",
            mount.kind
        );
    }
    if !mount.target.starts_with('/') || mount.target == "/" {
        bail!("RUN --mount needs target= an absolute path other than /");
    }
    Ok(mount)
}

/// `COPY [--chown= --chmod=] <<EOF [<<EOF2 ...] DEST` with its bodies: each
/// body becomes a file (named by its marker under a DEST ending in `/`, else
/// DEST itself), written by a RUN so nothing is uploaded. An unquoted marker
/// expands ARG/ENV variables, as in Docker; a quoted one keeps the body as is.
fn copy_heredoc(rest: &str, vars: &Lookup<'_>, plan: &mut Plan) -> Result<()> {
    use base64::Engine;
    let (head, body) = rest.split_once('\n').unwrap_or((rest, ""));
    let (flags, operands) = split_flags(head);
    let (mut owner, mut mode) = (String::new(), String::new());
    for flag in flags {
        match flag.split_once('=') {
            Some(("--chown", v)) => owner = v.to_string(),
            Some(("--chmod", v)) => mode = v.to_string(),
            _ if flag.starts_with("--link") => {}
            _ => bail!("COPY flag {flag} is not supported with a here-document"),
        }
    }
    let mut words: Vec<&str> = operands.split_whitespace().collect();
    let dest = words.pop().filter(|d| !d.starts_with("<<"));
    let Some(dest) = dest else {
        bail!("COPY needs a destination after the here-document");
    };
    let dest = substitute(dest, vars)?;
    let markers = heredoc_markers(&words.join(" "));
    if markers.len() != words.len() {
        bail!("COPY with a here-document takes only here-documents as sources");
    }
    if markers.len() > 1 && !dest.ends_with('/') {
        bail!("COPY of several here-documents needs a destination ending in /");
    }
    let mut lines = body.lines();
    for (word, (marker, strip_tabs)) in words.iter().zip(markers) {
        let mut text = String::new();
        loop {
            let line = lines
                .next()
                .ok_or_else(|| anyhow!("here-document {marker} is never closed"))?;
            let line = if strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if line == marker {
                break;
            }
            text.push_str(line);
            text.push('\n');
        }
        let quoted = word.contains('\'') || word.contains('"');
        if !quoted {
            text = substitute(&text, vars)?;
        }
        let target = if dest.ends_with('/') {
            format!("{dest}{marker}")
        } else {
            dest.clone()
        };
        let dir = target
            .rsplit_once('/')
            .map_or(".", |(d, _)| if d.is_empty() { "/" } else { d });
        let mut command = format!(
            "mkdir -p {} && echo '{}' | /bin/busybox base64 -d > {}",
            quote(dir),
            base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
            quote(&target)
        );
        if !mode.is_empty() {
            command.push_str(&format!(" && chmod {} {}", quote(&mode), quote(&target)));
        }
        if !owner.is_empty() {
            command.push_str(&format!(" && chown {} {}", quote(&owner), quote(&target)));
        }
        plan.steps.push(step("RUN", vec![command]));
    }
    Ok(())
}

/// The here-document markers on an instruction line, in order, and whether
/// each was `<<-` (leading tabs stripped from the terminator).
fn heredoc_markers(line: &str) -> Vec<(String, bool)> {
    line.split_whitespace()
        .filter_map(|word| {
            let rest = word.strip_prefix("<<")?;
            if rest.starts_with('<') {
                return None;
            }
            let (rest, strip_tabs) = match rest.strip_prefix('-') {
                Some(r) => (r, true),
                None => (rest, false),
            };
            let name = rest.trim_matches(|c| c == '\'' || c == '"');
            (!name.is_empty()).then(|| (name.to_string(), strip_tabs))
        })
        .collect()
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
    fn stages_are_parsed_one_each_and_copy_from_names_its_stage() {
        let plans = parse_stages(
            "ARG V=1\nFROM a AS build\nRUN make\nFROM b\nCOPY --from=build /out/app /app\n",
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].name.as_deref(), Some("build"));
        assert_eq!(plans[1].image, "b");
        let copy = &plans[1].steps[0];
        assert_eq!(copy.from_stage.as_deref(), Some("build"));
        assert!(!copy.from_context);
        assert!(!plans[1].copies_from_context());
    }

    #[test]
    fn a_stage_may_name_an_earlier_one_as_its_base_and_errors_keep_the_files_line() {
        let plans = parse_stages("FROM a AS x\nRUN ok\nFROM x\n", &BTreeMap::new()).unwrap();
        assert_eq!(
            (plans[0].name.as_deref(), plans[1].image.as_str()),
            (Some("x"), "x")
        );
        let message = parse_stages("FROM a\nRUN ok\nFROM b\nFROBNICATE x\n", &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(message.starts_with("Dockerfile line 4:"), "{message}");
    }

    #[test]
    fn what_cannot_be_done_is_refused_by_name() {
        for (text, expected) in [
            ("FROM a\nFROM b\n", "second FROM"),
            ("FROM a\nCOPY --from= /x /y\n", "needs a COPY and a stage"),
            ("FROM scratch\n", "scratch"),
            ("FROM a\nRUN --network=none make\n", "RUN flag"),
            ("FROM a\nRUN --mount=type=secret,id=x make\n", "type=secret"),
            ("FROM a\nRUN --mount=type=tmpfs make\n", "absolute path"),
            ("FROM a\nADD https://x.example/f /f\n", "ADD of"),
            ("FROM a\nADD <<EOF /x\nhi\nEOF\n", "here-document"),
            ("FROM a\nCOPY <<EOF\nhi\nEOF\n", "destination"),
            ("FROM a\nCOPY <<A <<B /d\na\nA\nb\nB\n", "ending in /"),
            ("FROM a\nRUN cat <<EOF\nx\n", "never closed"),
            ("FROM a\nSHELL /bin/bash -c\n", "SHELL needs a JSON array"),
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
    fn a_run_here_document_keeps_its_body_and_a_bare_one_runs_as_a_script() {
        let plan = parse(
            "FROM a\nRUN cat <<EOF >/f\nline one\n  two\nEOF\nRUN <<-X\n\tset -e\n\tX\nENV A=b\n",
            &BTreeMap::new(),
        )
        .unwrap();
        let runs: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.kind == "RUN")
            .map(|s| s.args[0].as_str())
            .collect();
        assert_eq!(
            runs,
            [
                "cat <<EOF >/f\nline one\n  two\nEOF",
                "/bin/sh <<-X\n\tset -e\n\tX"
            ]
        );
        assert!(plan.steps.iter().any(|s| s.kind == "ENV"));
    }

    #[test]
    fn run_mounts_last_for_one_run() {
        let plan = parse(
            "FROM a\nRUN --mount=type=bind,source=src,target=/b --mount=type=tmpfs,target=/t \
             make\nRUN --mount=type=cache,target=/c true\n",
            &BTreeMap::new(),
        )
        .unwrap();
        let kinds: Vec<&str> = plan.steps.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, ["COPY", "RUN", "RUN"]);
        let copy = &plan.steps[0];
        assert!(copy.from_context);
        assert_eq!(copy.args[..2], ["src", "/b"]);
        let run = &plan.steps[1].args[0];
        assert!(run.contains("{\nmake\n}; status=$?;"), "{run}");
        assert!(
            run.contains("rm -rf '/b';") && run.contains("rm -rf '/t'"),
            "{run}"
        );
        assert!(run.ends_with("exit $status"));
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains("cache"));
    }

    #[test]
    fn a_copy_here_document_is_written_by_a_run() {
        use base64::Engine;
        let decode = |s: &str| {
            let b64 = s
                .split("echo '")
                .nth(1)
                .unwrap()
                .split('\'')
                .next()
                .unwrap();
            String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap(),
            )
            .unwrap()
        };
        let plan = parse(
            "FROM a\nARG V=1\nCOPY --chmod=755 <<EOF /etc/a.conf\nv=$V\nEOF\n\
             COPY <<'Q' <<-R /out/\nraw $V\nQ\n\tx\n\tR\nENV A=b\n",
            &BTreeMap::new(),
        )
        .unwrap();
        let runs: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.kind == "RUN")
            .map(|s| s.args[0].as_str())
            .collect();
        assert_eq!(runs.len(), 3);
        assert!(plan.steps.iter().all(|s| s.kind != "COPY"));
        assert!(runs[0].starts_with("mkdir -p '/etc' && echo '"));
        assert!(runs[0].ends_with("> '/etc/a.conf' && chmod '755' '/etc/a.conf'"));
        assert_eq!(decode(runs[0]), "v=1\n");
        assert!(runs[1].ends_with("> '/out/Q'"));
        assert_eq!(decode(runs[1]), "raw $V\n");
        assert!(runs[2].ends_with("> '/out/R'"));
        assert_eq!(decode(runs[2]), "x\n");
        assert!(plan.steps.iter().any(|s| s.kind == "ENV"));
    }

    #[test]
    fn add_of_a_local_archive_unpacks_it_and_a_zip_is_copied_whole() {
        let plan = parse(
            "FROM a\nADD app.tar.gz /srv/app\nADD site.zip /srv/\n",
            &BTreeMap::new(),
        )
        .unwrap();
        let kinds: Vec<&str> = plan.steps.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, ["COPY", "RUN", "COPY"]);
        assert_eq!(plan.steps[0].args[1], "/tmp/.hm-add-0.tar");
        let run = &plan.steps[1].args[0];
        assert!(
            run.contains("mkdir -p '/srv/app'") && run.contains("rm -f /tmp/.hm-add-0.tar"),
            "{run}"
        );
        assert_eq!(plan.steps[2].args[0], "site.zip");
    }

    #[test]
    fn shell_changes_how_a_shell_form_run_and_cmd_are_run() {
        let plan = parse(
            "FROM a\nRUN one\nSHELL [\"/bin/sh\", \"-ec\"]\nRUN two words\nRUN [\"x\", \"y\"]\nCMD serve\n",
            &BTreeMap::new(),
        )
        .unwrap();
        let runs: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.kind == "RUN")
            .map(|s| s.args[0].as_str())
            .collect();
        assert_eq!(runs, ["one", "'/bin/sh' '-ec' 'two words'", "'x' 'y'"]);
        let cmd = plan.steps.iter().find(|s| s.kind == "CMD").unwrap();
        assert_eq!(cmd.args, ["/bin/sh", "-ec", "serve"]);
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
