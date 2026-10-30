//! `hm sandbox vm compose`: the pure half. Parsing a compose file, checking
//! it, and ordering its services; nothing here talks to a node.
//!
//! v1 covers `image`, `build`, `command`, `environment`, `depends_on` and
//! `ports` (each container port becomes a public-port reservation; the
//! gateway picks the public number, so a requested host port is not honoured).
//! `volumes` (named volumes only; a bind mount is refused). Anything else is
//! refused with an error rather than ignored, and
//! `depends_on` orders start-up; `condition: service_healthy` also waits for
//! the dependency's `healthcheck`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_yaml_ng::Value;

/// The longest name a sandbox may have.
const MAX_NAME: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    /// Accepted and ignored, as compose itself does.
    #[serde(default)]
    #[allow(dead_code)]
    version: Option<Value>,
    #[serde(default)]
    services: BTreeMap<String, RawService>,
    /// Named volumes. Only names: a body (driver, external, ...) is refused.
    #[serde(default)]
    volumes: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawService {
    image: Option<String>,
    build: Option<RawBuild>,
    command: Option<RawCommand>,
    environment: Option<RawEnv>,
    depends_on: Option<RawDepends>,
    ports: Option<Value>,
    volumes: Option<Value>,
    healthcheck: Option<RawHealth>,
    restart: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHealth {
    test: Option<RawCommand>,
    interval: Option<String>,
    retries: Option<u32>,
    #[serde(default)]
    disable: bool,
}

/// A `depends_on` entry's body: only `condition` is understood.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDepend {
    condition: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawBuild {
    Context(String),
    Full {
        context: String,
        #[serde(default)]
        dockerfile: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawCommand {
    Line(String),
    Words(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawEnv {
    Map(BTreeMap<String, Value>),
    List(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDepends {
    List(Vec<String>),
    Map(BTreeMap<String, Option<RawDepend>>),
}

/// Where a service's template comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Image(String),
    Build {
        context: String,
        dockerfile: Option<String>,
    },
}

/// A checked service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub source: Source,
    /// Empty means the image's own CMD.
    pub command: Vec<String>,
    /// `NAME=VALUE`, sorted by name.
    pub env: Vec<String>,
    pub depends_on: Vec<String>,
    /// Container ports to publish, by port number; the protocol is `tcp`,
    /// `udp`, or `both` when the file lists the port for each.
    pub ports: BTreeMap<u16, &'static str>,
    /// Named volumes to mount: `(volume, guest path)`, by path.
    pub mounts: Vec<(String, String)>,
    pub health: Option<Health>,
    /// The dependencies whose health this service waits for
    /// (`condition: service_healthy`); a subset of `depends_on`.
    pub wait_healthy: BTreeSet<String>,
}

/// A health check: `test` is an argv run in the guest every `interval_secs`;
/// exit 0 is healthy. Unhealthy after `retries` failures in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub test: Vec<String>,
    pub interval_secs: u64,
    pub retries: u32,
}

/// `5s`, `1m30s`, `2h`; compose's duration form, whole seconds only.
fn parse_duration(service: &str, text: &str) -> Result<u64> {
    let mut total = 0u64;
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => bail!("service {service:?}: healthcheck duration {text:?} must use s, m or h"),
        };
        let n: u64 = digits.parse().with_context(|| {
            format!("service {service:?}: healthcheck duration {text:?} is not valid")
        })?;
        digits.clear();
        total += n * unit;
    }
    if !digits.is_empty() || total == 0 {
        bail!("service {service:?}: healthcheck duration {text:?} must be like 5s or 1m30s");
    }
    Ok(total)
}

fn parse_health(service: &str, raw: Option<RawHealth>) -> Result<Option<Health>> {
    let Some(raw) = raw else { return Ok(None) };
    if raw.disable {
        return Ok(None);
    }
    let test = match raw.test {
        None => bail!("service {service:?}: healthcheck needs `test`"),
        Some(RawCommand::Line(l)) => vec!["sh".into(), "-c".into(), l],
        Some(RawCommand::Words(w)) => match w.first().map(String::as_str) {
            Some("NONE") => return Ok(None),
            Some("CMD") if w.len() > 1 => w[1..].to_vec(),
            Some("CMD-SHELL") if w.len() == 2 => vec!["sh".into(), "-c".into(), w[1].clone()],
            _ => bail!(
                "service {service:?}: healthcheck `test` list must start with CMD, CMD-SHELL or NONE"
            ),
        },
    };
    let interval_secs = match &raw.interval {
        Some(t) => parse_duration(service, t)?,
        None => 30,
    };
    let retries = raw.retries.unwrap_or(3);
    if retries == 0 {
        bail!("service {service:?}: healthcheck `retries` must be at least 1");
    }
    Ok(Some(Health {
        test,
        interval_secs,
        retries,
    }))
}

/// A name a volume can carry: what the node accepts, 1-64 of `[A-Za-z0-9_-]`.
fn valid_volume_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The volumes a compose file declares at its top level: names only.
fn parse_declared_volumes(value: &Option<Value>) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let map = match value {
        None | Some(Value::Null) => return Ok(out),
        Some(Value::Mapping(m)) => m,
        Some(_) => bail!("top-level `volumes:` must be a mapping of names"),
    };
    for (key, body) in map {
        let Value::String(name) = key else {
            bail!("top-level `volumes:` has a name that is not a string");
        };
        let empty = match body {
            Value::Null => true,
            Value::Mapping(m) => m.is_empty(),
            _ => false,
        };
        if !empty {
            bail!(
                "volume {name:?}: options (driver, external, ...) are not supported; \
                 declare it as `{name}:` with nothing under it"
            );
        }
        if !valid_volume_name(name) {
            bail!("volume name {name:?} must be 1-64 of letters, digits, `_` and `-`");
        }
        out.insert(name.clone());
    }
    Ok(out)
}

/// A service's `volumes:` list (short syntax, named volumes only) as
/// `(volume, guest path)`. Bind mounts, `:ro`, and volumes the file does not
/// declare are refused, not guessed at.
fn parse_mounts(
    service: &str,
    value: &Option<Value>,
    declared: &BTreeSet<String>,
) -> Result<Vec<(String, String)>> {
    let items = match value {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Sequence(items)) => items,
        Some(_) => bail!("service {service:?}: `volumes:` must be a list"),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for item in items {
        let Value::String(text) = item else {
            bail!(
                "service {service:?}: only the short `volumes:` syntax (\"data:/path\") is supported"
            );
        };
        let parts: Vec<&str> = text.split(':').collect();
        let (source, path) = match parts.as_slice() {
            [s, p] => (*s, *p),
            [s, p, "rw"] => (*s, *p),
            [_, _, mode] => bail!(
                "service {service:?}: volume mode {mode:?} in {text:?} is not supported \
                 (a mount is always read-write)"
            ),
            _ => bail!("service {service:?}: cannot read volume entry {text:?}"),
        };
        if source.starts_with('.') || source.starts_with('/') || source.starts_with('~') {
            bail!(
                "service {service:?}: {text:?} is a bind mount; a guest cannot see a host \
                 directory, use a named volume"
            );
        }
        if !declared.contains(source) {
            bail!(
                "service {service:?}: volume {source:?} is not declared under top-level `volumes:`"
            );
        }
        if !path.starts_with('/') || path == "/" {
            bail!("service {service:?}: guest path {path:?} in {text:?} must be absolute, not `/`");
        }
        if out.iter().any(|(_, p)| p == path) {
            bail!("service {service:?}: two volumes at {path:?}");
        }
        out.push((source.to_string(), path.to_string()));
    }
    Ok(out)
}

/// The name a compose volume has on the node: the project's, so two projects
/// can both declare `data`.
pub fn volume_name(project: &str, volume: &str) -> String {
    format!("{project}-{volume}")
}

/// Parse a compose `ports:` list (short syntax only) into container ports.
///
/// Accepted entries: `80`, `"8080:80"`, `"127.0.0.1:8080:80"`, and a
/// `/tcp` or `/udp` suffix. The host side is checked and dropped: the
/// gateway chooses the public port. Ranges and the long mapping syntax are
/// refused, not guessed at.
fn parse_ports(service: &str, value: &Option<Value>) -> Result<BTreeMap<u16, &'static str>> {
    let mut out: BTreeMap<u16, &'static str> = BTreeMap::new();
    let items = match value {
        None | Some(Value::Null) => return Ok(out),
        Some(Value::Sequence(items)) => items,
        Some(_) => bail!("service {service:?}: `ports:` must be a list"),
    };
    for item in items {
        let text = match item {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => bail!(
                "service {service:?}: only the short `ports:` syntax (\"8080:80\") is supported"
            ),
        };
        let (spec, protocol) = match text.rsplit_once('/') {
            Some((spec, "tcp")) => (spec, "tcp"),
            Some((spec, "udp")) => (spec, "udp"),
            Some((_, other)) => {
                bail!("service {service:?}: port protocol {other:?} is not tcp or udp")
            }
            None => (text.as_str(), "tcp"),
        };
        let parts: Vec<&str> = spec.split(':').collect();
        let (host, container) = match parts.as_slice() {
            [c] => (None, *c),
            [h, c] => (Some(*h), *c),
            [_ip, h, c] => (Some(*h), *c),
            _ => bail!("service {service:?}: cannot read port entry {text:?}"),
        };
        let number = |s: &str| -> Result<u16> {
            match s.parse::<u16>() {
                Ok(n) if n != 0 => Ok(n),
                _ => bail!(
                    "service {service:?}: port {s:?} in {text:?} is not a single port 1-65535 \
                     (ranges are not supported)"
                ),
            }
        };
        if let Some(host) = host.filter(|h| !h.is_empty()) {
            number(host)?;
        }
        let container = number(container)?;
        let merged = match out.get(&container) {
            Some(&seen) if seen != protocol => "both",
            _ => protocol,
        };
        out.insert(container, merged);
    }
    Ok(out)
}

/// The container ports of `hm sandbox vm run -p`, in the same forms as a
/// compose `ports:` entry (`80`, `8080:80`, `53/udp`).
pub fn parse_publish(specs: &[String]) -> Result<BTreeMap<u16, &'static str>> {
    let items = specs.iter().cloned().map(Value::String).collect();
    parse_ports("-p", &Some(Value::Sequence(items)))
}

/// A compose file that has been parsed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compose {
    /// By name.
    pub services: BTreeMap<String, Service>,
    /// Declared volume names (without the project prefix).
    pub volumes: BTreeSet<String>,
}

/// A project name a sandbox name can carry: lower case, `[a-z0-9_-]`, no
/// leading or trailing separator.
pub fn sanitize_project(raw: &str) -> Result<String> {
    let mapped: String = raw
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = mapped.trim_matches(|c| c == '-' || c == '_').to_string();
    if name.is_empty() {
        bail!("the project name {raw:?} has nothing usable in it; give one with -p");
    }
    Ok(name)
}

/// The sandbox name of a service.
pub fn vm_name(project: &str, service: &str) -> String {
    format!("{project}-{service}")
}

/// Split a command line into words, shell-style: whitespace separates,
/// single quotes keep everything, double quotes keep all but `\"` and `\\`,
/// and a backslash outside quotes escapes the next character.
pub fn split_command(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => bail!("command {line:?}: unterminated single quote"),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => bail!("command {line:?}: unterminated double quote"),
                        },
                        Some(c) => word.push(c),
                        None => bail!("command {line:?}: unterminated double quote"),
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => bail!("command {line:?}: ends in a backslash"),
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn scalar(service: &str, key: &str, value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => bail!(
            "service {service:?}: environment {key:?} needs a value; \
             inheriting it from the host is not supported"
        ),
    }
}

fn check_service_name(name: &str) -> Result<()> {
    let ok = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric());
    if !ok {
        bail!("service name {name:?}: use ASCII letters, digits, - and _, starting with a letter or digit");
    }
    Ok(())
}

/// Parse and check a compose file's text.
pub fn parse(text: &str) -> Result<Compose> {
    let raw: RawFile = serde_yaml_ng::from_str(text).context("parsing the compose file")?;
    if raw.services.is_empty() {
        bail!("the compose file defines no services");
    }
    let volumes = parse_declared_volumes(&raw.volumes)?;
    let mut services = BTreeMap::new();
    for (name, svc) in raw.services {
        check_service_name(&name)?;
        let ports = parse_ports(&name, &svc.ports)?;
        let mounts = parse_mounts(&name, &svc.volumes, &volumes)?;
        let source = match (svc.image, svc.build) {
            (Some(_), Some(_)) => bail!("service {name:?}: give `image` or `build`, not both"),
            (None, None) => bail!("service {name:?}: needs `image` or `build`"),
            (Some(image), None) if image.trim().is_empty() => {
                bail!("service {name:?}: `image` is empty")
            }
            (Some(image), None) => Source::Image(image),
            (None, Some(RawBuild::Context(context))) => Source::Build {
                context,
                dockerfile: None,
            },
            (
                None,
                Some(RawBuild::Full {
                    context,
                    dockerfile,
                }),
            ) => Source::Build {
                context,
                dockerfile,
            },
        };
        let command = match svc.command {
            None => Vec::new(),
            Some(RawCommand::Words(w)) => w,
            Some(RawCommand::Line(l)) => {
                split_command(&l).with_context(|| format!("service {name:?}"))?
            }
        };
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        match svc.environment {
            None => {}
            Some(RawEnv::Map(m)) => {
                for (k, v) in m {
                    let v = scalar(&name, &k, &v)?;
                    env.insert(k, v);
                }
            }
            Some(RawEnv::List(l)) => {
                for item in l {
                    let (k, v) = item.split_once('=').with_context(|| {
                        format!("service {name:?}: environment entry {item:?} is not NAME=VALUE")
                    })?;
                    env.insert(k.to_string(), v.to_string());
                }
            }
        }
        if let Some(bad) = env.keys().find(|k| k.is_empty()) {
            bail!("service {name:?}: environment name {bad:?} is empty");
        }
        // A service is never restarted after it exits, which is `restart: "no"`;
        // the other policies need a node that restarts sandboxes.
        match &svc.restart {
            None | Some(Value::Bool(false)) => {}
            Some(Value::String(s)) if s.is_empty() || s == "no" => {}
            Some(other) => bail!(
                "service {name:?}: restart: {other:?} is not supported (the node restarts \
                 machines, not sandboxes); only \"no\" is"
            ),
        }
        let health = parse_health(&name, svc.healthcheck)?;
        let mut wait_healthy = BTreeSet::new();
        let depends_on = match svc.depends_on {
            None => Vec::new(),
            Some(RawDepends::List(l)) => l,
            Some(RawDepends::Map(m)) => {
                for (dep, body) in &m {
                    match body.as_ref().and_then(|b| b.condition.as_deref()) {
                        None | Some("service_started") => {}
                        Some("service_healthy") => {
                            wait_healthy.insert(dep.clone());
                        }
                        Some(other) => bail!(
                            "service {name:?}: depends_on {dep:?} condition {other:?} is not supported \n                             (service_started, service_healthy)"
                        ),
                    }
                }
                m.into_keys().collect()
            }
        };
        services.insert(
            name.clone(),
            Service {
                name,
                source,
                command,
                env: env.into_iter().map(|(k, v)| format!("{k}={v}")).collect(),
                depends_on,
                ports,
                mounts,
                health,
                wait_healthy,
            },
        );
    }
    let compose = Compose { services, volumes };
    compose.start_order()?;
    Ok(compose)
}

impl Compose {
    /// Service names in start order: every service after those it depends
    /// on, ties broken by name so the order is the same every time. Fails on
    /// a dependency that is not a service and on a cycle.
    pub fn start_order(&self) -> Result<Vec<String>> {
        for svc in self.services.values() {
            for dep in &svc.wait_healthy {
                if self.services.get(dep).is_some_and(|d| d.health.is_none()) {
                    bail!(
                        "service {:?} waits for {dep:?} to be healthy, which has no healthcheck",
                        svc.name
                    );
                }
            }
        }
        for svc in self.services.values() {
            for dep in &svc.depends_on {
                if !self.services.contains_key(dep) {
                    bail!(
                        "service {:?} depends_on {dep:?}, which is not a service in the file",
                        svc.name
                    );
                }
            }
        }
        let mut waiting: BTreeMap<&str, BTreeSet<&str>> = self
            .services
            .values()
            .map(|s| {
                (
                    s.name.as_str(),
                    s.depends_on.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        let mut order = Vec::with_capacity(waiting.len());
        while !waiting.is_empty() {
            let Some(next) = waiting
                .iter()
                .find(|(_, deps)| deps.is_empty())
                .map(|(name, _)| *name)
            else {
                let stuck: Vec<&str> = waiting.keys().copied().collect();
                bail!("depends_on has a cycle among: {}", stuck.join(", "));
            };
            waiting.remove(next);
            for deps in waiting.values_mut() {
                deps.remove(next);
            }
            order.push(next.to_string());
        }
        Ok(order)
    }

    /// Check that every sandbox name `project` gives a service is one the
    /// node accepts.
    pub fn check_names(&self, project: &str) -> Result<()> {
        for name in &self.volumes {
            let full = volume_name(project, name);
            if full.len() > MAX_NAME {
                bail!(
                    "volume name {full:?} is {} bytes; the limit is {MAX_NAME}. \
                     Shorten the project (-p) or the volume name",
                    full.len()
                );
            }
        }
        for name in self.services.keys() {
            let full = vm_name(project, name);
            if full.len() > MAX_NAME {
                bail!(
                    "sandbox name {full:?} is {} bytes; the limit is {MAX_NAME}. \
                     Shorten the project (-p) or the service name",
                    full.len()
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    #[test]
    fn command_and_environment_in_both_forms() {
        let c = parse(
            r#"
services:
  a:
    image: alpine
    command: sh -c 'echo "hi there"'
    environment:
      B: two
      A: 1
      C: true
  b:
    image: alpine
    command: ["sleep", "10"]
    environment:
      - X=1=2
      - Y=
"#,
        )
        .unwrap();
        let a = &c.services["a"];
        assert_eq!(a.command, ["sh", "-c", "echo \"hi there\""]);
        assert_eq!(a.env, ["A=1", "B=two", "C=true"]);
        let b = &c.services["b"];
        assert_eq!(b.command, ["sleep", "10"]);
        assert_eq!(b.env, ["X=1=2", "Y="]);
    }

    #[test]
    fn build_in_both_forms() {
        let c = parse(
            "services:\n  a:\n    build: ./app\n  b:\n    build:\n      context: ./b\n      dockerfile: Dockerfile.dev\n",
        )
        .unwrap();
        assert_eq!(
            c.services["a"].source,
            Source::Build {
                context: "./app".into(),
                dockerfile: None
            }
        );
        assert_eq!(
            c.services["b"].source,
            Source::Build {
                context: "./b".into(),
                dockerfile: Some("Dockerfile.dev".into())
            }
        );
    }

    #[test]
    fn image_xor_build() {
        assert!(err("services:\n  a: {}\n").contains("needs `image` or `build`"));
        assert!(err("services:\n  a: {image: x, build: .}\n").contains("not both"));
    }

    #[test]
    fn splits_like_a_shell() {
        assert_eq!(split_command("a  b\tc").unwrap(), ["a", "b", "c"]);
        assert_eq!(
            split_command(r#"a "b c" 'd e' f\ g"#).unwrap(),
            ["a", "b c", "d e", "f g"]
        );
        assert_eq!(split_command("''").unwrap(), [""]);
        assert!(split_command("a 'b").is_err());
        assert!(split_command("a \"b").is_err());
        assert!(split_command("a\\").is_err());
    }

    fn three() -> &'static str {
        r#"
services:
  web:
    image: x
    depends_on: [api, db]
  api:
    image: x
    depends_on:
      db:
        condition: service_started
  db:
    image: x
"#
    }

    #[test]
    fn orders_dependencies_first() {
        let c = parse(three()).unwrap();
        assert_eq!(c.start_order().unwrap(), ["db", "api", "web"]);
    }

    #[test]
    fn independent_services_order_by_name() {
        let c = parse("services:\n  b: {image: x}\n  a: {image: x}\n").unwrap();
        assert_eq!(c.start_order().unwrap(), ["a", "b"]);
    }

    #[test]
    fn cycle_is_an_error() {
        let e = err("services:\n  a: {image: x, depends_on: [b]}\n  b: {image: x, depends_on: [a]}\n  c: {image: x}\n");
        assert!(e.contains("cycle"), "{e}");
        assert!(e.contains("a, b") && !e.contains(", c"), "{e}");
        assert!(err("services:\n  a: {image: x, depends_on: [a]}\n").contains("cycle"));
    }

    #[test]
    fn unknown_dependency_is_an_error() {
        let e = err("services:\n  a: {image: x, depends_on: [ghost]}\n");
        assert!(e.contains("ghost") && e.contains("not a service"), "{e}");
    }

    #[test]
    fn named_volumes_become_mounts() {
        let c = parse(
            "services:\n  a:\n    image: x\n    volumes: [\"data:/var/lib/x\", \"logs:/logs:rw\"]\n\
             volumes:\n  data:\n  logs: {}\n",
        )
        .unwrap();
        assert_eq!(
            c.services["a"].mounts,
            [
                ("data".to_string(), "/var/lib/x".to_string()),
                ("logs".to_string(), "/logs".to_string())
            ]
        );
        assert_eq!(c.volumes.len(), 2);
        assert_eq!(volume_name("proj", "data"), "proj-data");
        // Empty lists say nothing and are fine.
        assert!(parse("services:\n  a:\n    image: x\n    volumes: []\n").is_ok());
    }

    #[test]
    fn volumes_that_cannot_work_are_errors() {
        let svc = |entry: &str, top: &str| {
            err(&format!(
                "services:\n  a:\n    image: x\n    volumes: [{entry}]\n{top}"
            ))
        };
        let declared = "volumes:\n  data:\n";
        assert!(svc("\"./d:/d\"", declared).contains("bind mount"));
        assert!(svc("\"/srv/d:/d\"", declared).contains("bind mount"));
        assert!(svc("\"data:/d\"", "").contains("not declared"));
        assert!(svc("\"data:/d:ro\"", declared).contains("mode"));
        assert!(svc("\"data:rel\"", declared).contains("absolute"));
        assert!(svc("\"data:/\"", declared).contains("absolute"));
        assert!(svc("\"data:/d\", \"data:/d\"", declared).contains("two volumes"));
        assert!(svc("{type: volume}", declared).contains("short"));
        let e = err("services:\n  a: {image: x}\nvolumes:\n  data: {driver: local}\n");
        assert!(e.contains("options"), "{e}");
        let e = err("services:\n  a: {image: x}\nvolumes:\n  \"bad.name\":\n");
        assert!(e.contains("bad.name"), "{e}");
    }

    #[test]
    fn a_long_project_name_overflows_a_volume_name() {
        let c = parse("services:\n  a: {image: x}\nvolumes:\n  data:\n").unwrap();
        assert!(c.check_names("proj").is_ok());
        let long = "p".repeat(62);
        let e = format!("{:#}", c.check_names(&long).unwrap_err());
        assert!(e.contains("volume name"), "{e}");
    }

    fn ports_of(list: &str) -> BTreeMap<u16, &'static str> {
        let text = format!("services:\n  a:\n    image: x\n    ports: {list}\n");
        parse(&text).unwrap().services["a"].ports.clone()
    }

    #[test]
    fn run_publish_takes_the_compose_port_forms_and_refuses_ranges() {
        let given =
            |v: &[&str]| parse_publish(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let got = given(&["8080:80", "53/udp", "53"]).unwrap();
        assert_eq!(got.get(&80), Some(&"tcp"));
        assert_eq!(got.get(&53), Some(&"both"));
        assert!(given(&[]).unwrap().is_empty());
        assert!(given(&["8000-8010:80"]).is_err());
        assert!(given(&["0"]).is_err());
    }

    #[test]
    fn ports_keep_the_container_side_and_the_protocol() {
        assert!(ports_of("[]").is_empty());
        let got = ports_of("[80, \"8080:81\", \"127.0.0.1:9000:82\", \"53:53/udp\"]");
        assert_eq!(
            got,
            BTreeMap::from([(53, "udp"), (80, "tcp"), (81, "tcp"), (82, "tcp")])
        );
        // The same port for both protocols is one `both` reservation.
        assert_eq!(
            ports_of("[\"53:53/udp\", \"53:53\"]"),
            BTreeMap::from([(53, "both")])
        );
    }

    #[test]
    fn ports_that_cannot_be_published_faithfully_are_errors() {
        for bad in [
            "[\"8000-8010:80\"]",
            "[\"0\"]",
            "[\"70000\"]",
            "[\"80/sctp\"]",
            "[\"a:b:c:d\"]",
        ] {
            let e = err(&format!(
                "services:\n  a:\n    image: x\n    ports: {bad}\n"
            ));
            assert!(e.contains("service \"a\""), "{bad}: {e}");
        }
        let e = err("services:\n  a:\n    image: x\n    ports: [{target: 80}]\n");
        assert!(e.contains("short `ports:` syntax"), "{e}");
    }

    #[test]
    fn healthchecks_and_service_healthy_conditions() {
        let c = parse(
            "services:
  db:
    image: x
    healthcheck:
      test: [CMD-SHELL, \"true\"]
      interval: 1m30s
      retries: 5
  app:
    image: x
    depends_on:
      db:
        condition: service_healthy
",
        )
        .unwrap();
        let h = c.services["db"].health.clone().unwrap();
        assert_eq!(h.test, vec!["sh", "-c", "true"]);
        assert_eq!((h.interval_secs, h.retries), (90, 5));
        assert!(c.services["app"].wait_healthy.contains("db"));
        let c = parse(
            "services:
  a: {image: x, healthcheck: {test: [CMD, ls, /]}}
",
        )
        .unwrap();
        assert_eq!(
            c.services["a"].health.as_ref().unwrap().test,
            vec!["ls", "/"]
        );
        assert_eq!(c.services["a"].health.as_ref().unwrap().interval_secs, 30);
        assert!(err("services:
  a: {image: x}
  b: {image: x, depends_on: {a: {condition: service_healthy}}}
")
        .contains("no healthcheck"));
        assert!(err("services:
  a: {image: x, depends_on: {b: {condition: service_completed_successfully}}}
  b: {image: x}
")
        .contains("not supported"));
        assert!(err("services:
  a: {image: x, healthcheck: {test: [FOO]}}
")
        .contains("CMD"));
        assert!(err("services:
  a: {image: x, healthcheck: {test: 'true', interval: 5}}
")
        .contains("duration"));
    }

    #[test]
    fn unknown_keys_and_bad_environment_are_errors() {
        assert!(err("services:\n  a: {image: x, restart: always}\n").contains("restart"));
        assert!(err("services:\n  a: {image: x, restart: on-failure}\n").contains("not supported"));
        for ok in ["\"no\"", "no", "false", "\"\""] {
            parse(&format!("services:\n  a: {{image: x, restart: {ok}}}\n"))
                .unwrap_or_else(|e| panic!("restart: {ok}: {e}"));
        }
        assert!(err("services:\n  a: {image: x, environment: [NOEQUALS]}\n").contains("NAME=VALUE"));
        assert!(
            err("services:\n  a: {image: x, environment: {K: null}}\n").contains("needs a value")
        );
        assert!(err("services: {}\n").contains("no services"));
    }

    #[test]
    fn project_names_are_sanitised() {
        assert_eq!(sanitize_project("My App").unwrap(), "my-app");
        assert_eq!(sanitize_project("demo_1").unwrap(), "demo_1");
        assert_eq!(sanitize_project("..web.site..").unwrap(), "web-site");
        assert_eq!(sanitize_project("Ünï").unwrap(), "n");
        assert!(sanitize_project("...").is_err());
        assert!(sanitize_project("").is_err());
    }

    #[test]
    fn long_sandbox_names_are_refused() {
        let c = parse("services:\n  web: {image: x}\n").unwrap();
        assert!(c.check_names("proj").is_ok());
        assert!(c.check_names(&"p".repeat(62)).is_err());
        assert_eq!(vm_name("proj", "web"), "proj-web");
    }
}
