use std::net::Ipv4Addr;
use std::time::Duration;

use crate::http::HARD_MAX_BODY;

pub const DEFAULT_MAX_BODY: usize = 262_144;
pub const DEFAULT_TIMEOUT_SECS: u64 = 15;
pub const MAX_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

pub const USAGE: &str = "usage: hv2-urltrace <URL> [--max-body BYTES] [--timeout SECS] [--user-agent STRING] [--resolve IPV4]";

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub max_body: usize,
    pub timeout: Duration,
    pub user_agent: String,
    pub resolve: Option<Ipv4Addr>,
    pub clamped: Vec<String>,
}

impl Config {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            max_body: DEFAULT_MAX_BODY,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            resolve: None,
            clamped: Vec::new(),
        }
    }

    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut url: Option<String> = None;
        let mut max_body: Option<usize> = None;
        let mut timeout: Option<u64> = None;
        let mut user_agent: Option<String> = None;
        let mut resolve: Option<Ipv4Addr> = None;
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
            match arg.as_str() {
                "--max-body" => {
                    let v = value("--max-body")?;
                    let n = v
                        .parse::<usize>()
                        .map_err(|_| format!("--max-body: {v:?} is not a byte count"))?;
                    if max_body.replace(n).is_some() {
                        return Err("--max-body given twice".into());
                    }
                }
                "--timeout" => {
                    let v = value("--timeout")?;
                    let n = v.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                        format!("--timeout: {v:?} is not a positive number of seconds")
                    })?;
                    if timeout.replace(n).is_some() {
                        return Err("--timeout given twice".into());
                    }
                }
                "--user-agent" => {
                    let v = value("--user-agent")?;
                    if v.is_empty() || v.bytes().any(|b| b < 0x20 || b == 0x7f) {
                        return Err(
                            "--user-agent must be non-empty without control characters".into()
                        );
                    }
                    if user_agent.replace(v).is_some() {
                        return Err("--user-agent given twice".into());
                    }
                }
                "--resolve" => {
                    let v = value("--resolve")?;
                    let ip = v
                        .parse::<Ipv4Addr>()
                        .map_err(|_| format!("--resolve: {v:?} is not an IPv4 address"))?;
                    if resolve.replace(ip).is_some() {
                        return Err("--resolve given twice".into());
                    }
                }
                s if s.starts_with("--") => return Err(format!("unknown option {s}")),
                _ => {
                    if url.replace(arg).is_some() {
                        return Err("more than one URL given".into());
                    }
                }
            }
        }
        let url = url.ok_or_else(|| "no URL given".to_owned())?;
        let mut cfg = Self::new(&url);
        if let Some(n) = max_body {
            cfg.max_body = n;
            if n > HARD_MAX_BODY {
                cfg.max_body = HARD_MAX_BODY;
                cfg.clamped.push(format!(
                    "--max-body {n} reduced to the hard cap {HARD_MAX_BODY}"
                ));
            }
        }
        if let Some(n) = timeout {
            let n = if n > MAX_TIMEOUT_SECS {
                cfg.clamped.push(format!(
                    "--timeout {n} reduced to the cap {MAX_TIMEOUT_SECS}"
                ));
                MAX_TIMEOUT_SECS
            } else {
                n
            };
            cfg.timeout = Duration::from_secs(n);
        }
        if let Some(ua) = user_agent {
            cfg.user_agent = ua;
        }
        cfg.resolve = resolve;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config, String> {
        Config::from_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn defaults_and_options() {
        let c = parse(&["http://a.test/"]).unwrap();
        assert_eq!(c.max_body, DEFAULT_MAX_BODY);
        assert_eq!(c.timeout, Duration::from_secs(15));
        assert!(c.user_agent.contains("Chrome/"));
        let c = parse(&[
            "--max-body",
            "100",
            "http://a.test/",
            "--timeout",
            "3",
            "--user-agent",
            "x",
            "--resolve",
            "10.0.0.1",
        ])
        .unwrap();
        assert_eq!(c.max_body, 100);
        assert_eq!(c.timeout, Duration::from_secs(3));
        assert_eq!(c.user_agent, "x");
        assert_eq!(c.resolve, Some(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn clamps_and_rejects() {
        let c = parse(&["u", "--max-body", "999999999", "--timeout", "9999"]).unwrap();
        assert_eq!(c.max_body, HARD_MAX_BODY);
        assert_eq!(c.timeout, Duration::from_secs(MAX_TIMEOUT_SECS));
        assert_eq!(c.clamped.len(), 2);
        for bad in [
            &[][..],
            &["a", "b"],
            &["a", "--nope"],
            &["a", "--timeout"],
            &["a", "--timeout", "0"],
            &["a", "--max-body", "-1"],
            &["a", "--resolve", "::1"],
            &["a", "--user-agent", "a\r\nb"],
            &["a", "--timeout", "1", "--timeout", "2"],
        ] {
            assert!(parse(bad).is_err(), "accepted {bad:?}");
        }
    }
}
