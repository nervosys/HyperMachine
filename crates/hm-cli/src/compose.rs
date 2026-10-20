//! `hm sandbox vm compose`: the pure half. Parsing a compose file, checking
//! it, and ordering its services; nothing here talks to a node.
//!
//! v1 covers `image`, `build`, `command`, `environment` and `depends_on`.
//! `ports` and `volumes` are refused with an error rather than ignored, and
//! services cannot reach one another over a network yet: `depends_on` only
//! orders start-up.

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
    Map(BTreeMap<String, Value>),
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
}

/// A compose file that has been parsed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compose {
    /// By name.
    pub services: BTreeMap<String, Service>,
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

fn refuse_nonempty(service: &str, key: &str, value: &Option<Value>) -> Result<()> {
    let empty = match value {
        None | Some(Value::Null) => true,
        Some(Value::Sequence(s)) => s.is_empty(),
        Some(Value::Mapping(m)) => m.is_empty(),
        Some(_) => false,
    };
    if !empty {
        bail!(
            "service {service:?}: `{key}:` is not supported yet by `hm sandbox vm compose` \
             (v1 has no published ports, mounts, or service-to-service networking)"
        );
    }
    Ok(())
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
    let mut services = BTreeMap::new();
    for (name, svc) in raw.services {
        check_service_name(&name)?;
        refuse_nonempty(&name, "ports", &svc.ports)?;
        refuse_nonempty(&name, "volumes", &svc.volumes)?;
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
        let depends_on = match svc.depends_on {
            None => Vec::new(),
            Some(RawDepends::List(l)) => l,
            Some(RawDepends::Map(m)) => m.into_keys().collect(),
        };
        services.insert(
            name.clone(),
            Service {
                name,
                source,
                command,
                env: env.into_iter().map(|(k, v)| format!("{k}={v}")).collect(),
                depends_on,
            },
        );
    }
    let compose = Compose { services };
    compose.start_order()?;
    Ok(compose)
}

impl Compose {
    /// Service names in start order: every service after those it depends
    /// on, ties broken by name so the order is the same every time. Fails on
    /// a dependency that is not a service and on a cycle.
    pub fn start_order(&self) -> Result<Vec<String>> {
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
    fn ports_and_volumes_are_refused() {
        let e = err("services:\n  a:\n    image: x\n    ports: [\"8080:80\"]\n");
        assert!(e.contains("`ports:` is not supported yet"), "{e}");
        let e = err("services:\n  a:\n    image: x\n    volumes: [\"./d:/d\"]\n");
        assert!(e.contains("`volumes:` is not supported yet"), "{e}");
        // Empty ones say nothing and are fine.
        assert!(parse("services:\n  a:\n    image: x\n    ports: []\n").is_ok());
    }

    #[test]
    fn unknown_keys_and_bad_environment_are_errors() {
        assert!(err("services:\n  a: {image: x, restart: always}\n").contains("restart"));
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
