//! Masking the common shapes of a secret in what a command printed.
//!
//! A command line is masked as it is kept ([`super::redact`]); output is
//! masked as it is read, so that the offsets a caller reads from stay the
//! stream's own. What is kept is whole; what is returned is not.
//!
//! The shapes: a private key's PEM block; `NAME=VALUE`, `name: value` and
//! `"name": "value"` for a name holding a secret word (as the command line's
//! rule, so `env`, `printenv`, `cat .env` and a JSON config are covered); the
//! credential after `Bearer`, `Basic`, `Token` or `Digest` in a header; a URL
//! with credentials; and a few tokens by their shape (AWS, GitHub, GitLab,
//! Slack, Stripe, Google, npm, JWT, `sk-` API keys). It is a line at a time,
//! except for a key block and a quoted value that runs on.

use super::{secret_name, MASK};

/// `text`, with the common shapes of a secret masked; its lines and spacing
/// kept.
pub(crate) fn redact(text: &str) -> String {
    let text = private_keys(text);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (body, end) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        out.push_str(&one_line(body));
        out.push_str(end);
    }
    out
}

/// A PEM private key, header to footer (or to the end of the text, when its
/// end was cut off), as one `[redacted private key]`.
fn private_keys(text: &str) -> String {
    const BEGIN: &str = "-----BEGIN ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let label_from = start + BEGIN.len();
        let Some(label_len) = rest[label_from..].find("-----") else {
            break;
        };
        if !rest[label_from..label_from + label_len].contains("PRIVATE KEY") {
            out.push_str(&rest[..label_from]);
            rest = &rest[label_from..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str("[redacted private key]");
        let body = label_from + label_len + 5;
        rest = match rest[body..].find("-----END ") {
            Some(end) => {
                let footer = body + end + "-----END ".len();
                match rest[footer..].find("-----") {
                    Some(close) => &rest[footer + close + 5..],
                    None => "",
                }
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

const TAIL: [char; 8] = ['"', '\'', ',', ';', ')', ']', '}', '>'];

fn is_scheme(word: &str) -> bool {
    matches!(
        word.trim_matches(|c| c == '"' || c == '\'')
            .to_ascii_lowercase()
            .as_str(),
        "bearer" | "basic" | "token" | "digest"
    )
}

fn one_line(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    // The next word is a value: after `name:` for a secret name, or after
    // `Bearer`.
    let mut value_next = false;
    let mut in_quote: Option<char> = None;
    loop {
        let Some(start) = rest.find(|c: char| !c.is_whitespace()) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        rest = &rest[end..];
        if let Some(quote) = in_quote {
            // The rest of a quoted value begun in an earlier word.
            if word.trim_end_matches(TAIL).ends_with(quote) || word == quote.to_string() {
                in_quote = None;
            }
            out.truncate(out.trim_end().len());
            continue;
        }
        if value_next {
            value_next = false;
            out.push_str(&value(word, &mut in_quote, &mut value_next));
        } else {
            out.push_str(&other(word, &mut in_quote, &mut value_next));
        }
    }
}

/// A word that is a secret's value, masked; the quotes and the punctuation
/// around it kept. A scheme word (`Bearer`) is kept, and the word after it
/// is the value.
fn value(word: &str, in_quote: &mut Option<char>, value_next: &mut bool) -> String {
    if is_scheme(word) {
        *value_next = true;
        return word.to_string();
    }
    let prefix: String = word
        .chars()
        .take_while(|c| *c == '"' || *c == '\'')
        .collect();
    let inner = &word[prefix.len()..];
    let suffix: String = inner
        .chars()
        .rev()
        .take_while(|c| TAIL.contains(c))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if inner.len() == suffix.len() {
        // Nothing but quotes or punctuation: there is no value in it.
        return word.to_string();
    }
    // A quote the word opens and does not close, closing in a later word.
    if let Some(quote) = prefix.chars().next() {
        if !inner.contains(quote) {
            *in_quote = Some(quote);
            return format!("{prefix}{MASK}");
        }
    }
    format!("{prefix}{MASK}{suffix}")
}

/// A word that is not known to be a value.
fn other(word: &str, in_quote: &mut Option<char>, value_next: &mut bool) -> String {
    // Credentials in a URL: scheme://user:password@host
    if let Some(at_scheme) = word.find("://") {
        let after = at_scheme + 3;
        let host_end = word[after..].find('/').map_or(word.len(), |i| after + i);
        if let Some(at) = word[after..host_end].rfind('@') {
            return format!("{}{MASK}{}", &word[..after], &word[after + at..]);
        }
    }
    // NAME=VALUE.
    if let Some((name, val)) = word.split_once('=') {
        if secret_name(name.trim_matches(|c| c == '"' || c == '\'')) && !val.is_empty() {
            return format!("{name}={}", value(val, in_quote, value_next));
        }
    }
    // `name:` or `"name":`, the value in the next word.
    if let Some(name) = word.strip_suffix(':') {
        if secret_name(name.trim_matches(|c| c == '"' || c == '\'')) {
            *value_next = true;
            return word.to_string();
        }
    }
    // A token by its shape, inside whatever punctuation surrounds it.
    let core = word.trim_matches(|c: char| c == '"' || c == '\'' || TAIL.contains(&c) || c == '(');
    if !core.is_empty() && token_shape(core) {
        return word.replacen(core, MASK, 1);
    }
    word.to_string()
}

fn all(text: &str, ok: impl Fn(u8) -> bool) -> bool {
    text.bytes().all(ok)
}

fn urlsafe(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Whether `core` is a well-known kind of token by its prefix and length.
fn token_shape(core: &str) -> bool {
    if (core.starts_with("AKIA") || core.starts_with("ASIA")) && core.len() == 20 {
        return all(&core[4..], |c| c.is_ascii_uppercase() || c.is_ascii_digit());
    }
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        if let Some(rest) = core.strip_prefix(prefix) {
            return rest.len() >= 36 && all(rest, |c| c.is_ascii_alphanumeric());
        }
    }
    if let Some(rest) = core.strip_prefix("github_pat_") {
        return rest.len() >= 40 && all(rest, urlsafe);
    }
    if let Some(rest) = core.strip_prefix("glpat-") {
        return rest.len() >= 20 && all(rest, urlsafe);
    }
    if let Some(rest) = core.strip_prefix("xox") {
        let mut chars = rest.chars();
        let kind = chars.next();
        return matches!(kind, Some('a' | 'b' | 'p' | 'r' | 's'))
            && chars.next() == Some('-')
            && rest.len() >= 12
            && all(&rest[2..], urlsafe);
    }
    for prefix in ["sk_live_", "rk_live_"] {
        if let Some(rest) = core.strip_prefix(prefix) {
            return rest.len() >= 16 && all(rest, |c| c.is_ascii_alphanumeric());
        }
    }
    if let Some(rest) = core.strip_prefix("sk-") {
        return rest.len() >= 32 && all(rest, urlsafe);
    }
    if let Some(rest) = core.strip_prefix("AIza") {
        return rest.len() == 35 && all(rest, urlsafe);
    }
    if let Some(rest) = core.strip_prefix("npm_") {
        return rest.len() == 36 && all(rest, |c| c.is_ascii_alphanumeric());
    }
    if core.starts_with("eyJ") {
        let parts: Vec<&str> = core.split('.').collect();
        return parts.len() == 3
            && parts[0].len() >= 10
            && parts[1].len() >= 10
            && parts[2].len() >= 8
            && parts.iter().all(|p| all(p, urlsafe));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_dump_keeps_its_names_and_masks_secret_values() {
        let env = "HOME=/root\nAPI_TOKEN=abc123\nPATH=/bin\nDB_PASSWORD=hunter2\nMONKEY=1\n";
        assert_eq!(
            redact(env),
            "HOME=/root\nAPI_TOKEN=[redacted]\nPATH=/bin\nDB_PASSWORD=[redacted]\nMONKEY=[redacted]\n"
        );
    }

    #[test]
    fn json_and_yaml_fields_are_masked_whole() {
        assert_eq!(
            redact("{\"user\": \"ann\", \"password\": \"s3cret\", \"n\": 1}"),
            "{\"user\": \"ann\", \"password\": \"[redacted]\", \"n\": 1}"
        );
        assert_eq!(
            redact("api_key: abc\nname: x\n"),
            "api_key: [redacted]\nname: x\n"
        );
        assert_eq!(redact("secret: 'two words'"), "secret: '[redacted]");
    }

    #[test]
    fn a_header_masks_the_credential_after_its_scheme() {
        assert_eq!(
            redact("Authorization: Bearer abc.def.ghi\nAccept: */*\n"),
            "Authorization: Bearer [redacted]\nAccept: */*\n"
        );
        assert_eq!(
            redact("authorization: Basic dXNlcjpwdw=="),
            "authorization: Basic [redacted]"
        );
    }

    #[test]
    fn prose_with_those_words_is_left_alone() {
        let prose = "Basic usage: run it\nThe token bucket refills\nBearer of news\n";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn a_url_with_credentials_loses_them() {
        assert_eq!(
            redact("cloning https://ann:pw@github.com/org/repo.git ..."),
            "cloning https://[redacted]@github.com/org/repo.git ..."
        );
        assert_eq!(
            redact("https://github.com/org/repo"),
            "https://github.com/org/repo"
        );
    }

    #[test]
    fn a_private_key_block_goes_whole_and_a_cut_one_to_the_end() {
        let block = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\nabcd\n-----END RSA PRIVATE KEY-----\nafter\n";
        assert_eq!(redact(block), "before\n[redacted private key]\nafter\n");
        assert_eq!(
            redact("x\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl\nbmFt"),
            "x\n[redacted private key]"
        );
        // A public key or certificate is not a secret.
        let cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        assert_eq!(redact(cert), cert);
    }

    #[test]
    fn well_known_tokens_are_masked_by_their_shape() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVP";
        for token in [
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "xoxb-1234567890-abcdefghij",
            "sk_live_abcdefghijklmnop1234",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
            "AIzaSyA-abcdefghijklmnopqrstuvwxyz01234",
            jwt,
        ] {
            let line = format!("found {token} in the log");
            assert_eq!(redact(&line), "found [redacted] in the log", "{token}");
            let quoted = format!("(\"{token}\"),");
            assert_eq!(redact(&quoted), "(\"[redacted]\"),", "{token}");
        }
        // Near misses are left alone.
        for text in [
            "AKIA123",
            "ghp_short",
            "sk-short",
            "eyJ.a.b",
            "xoxz-1234567890-abc",
        ] {
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn spacing_line_endings_and_non_ascii_survive() {
        let text = "a  b\t\tPASSWORD=x  \r\nпривет: мир\r\n\n";
        assert_eq!(
            redact(text),
            "a  b\t\tPASSWORD=[redacted]  \r\nпривет: мир\r\n\n"
        );
    }

    #[test]
    fn text_with_nothing_to_mask_comes_back_as_it_was() {
        let text = "Compiling hv2 v1.0\n    Finished release in 4.2s\n";
        assert_eq!(redact(text), text);
        assert_eq!(redact(""), "");
    }
}
