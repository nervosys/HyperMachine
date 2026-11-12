use serde::Serialize;

pub const MAX_META_HINTS: usize = 5;
pub const MAX_SCRIPT_HINTS: usize = 5;
pub const SCRIPT_CONTEXT: usize = 200;
const META_TAG_LIMIT: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hint {
    pub kind: String,
    pub value: String,
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn trim_value(s: &[u8]) -> &[u8] {
    let is_junk = |b: &u8| b.is_ascii_whitespace() || *b == b'"' || *b == b'\'' || *b == b';';
    let mut s = s;
    while let [first, rest @ ..] = s {
        if is_junk(first) {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if is_junk(last) {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn attribute_value<'a>(tag: &'a [u8], lower: &[u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut from = 0;
    while let Some(at) = find(lower, name, from) {
        from = at + name.len();
        let mut i = at + name.len();
        while lower.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if lower.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while lower.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let rest = tag.get(i..)?;
        return match rest.first() {
            Some(q @ (b'"' | b'\'')) => {
                let inner = rest.get(1..)?;
                let end = inner.iter().position(|b| b == q).unwrap_or(inner.len());
                inner.get(..end)
            }
            _ => {
                let end = rest
                    .iter()
                    .position(|b| b.is_ascii_whitespace() || *b == b'>')
                    .unwrap_or(rest.len());
                rest.get(..end)
            }
        };
    }
    None
}

fn meta_refresh(body: &[u8], lower_body: &[u8], out: &mut Vec<Hint>) {
    let mut from = 0;
    let mut found = 0;
    while found < MAX_META_HINTS {
        let Some(start) = find(lower_body, b"<meta", from) else {
            break;
        };
        from = start + 5;
        let window_end = (start + META_TAG_LIMIT).min(lower_body.len());
        let region = lower_body.get(start..window_end).unwrap_or(&[]);
        let end = region
            .iter()
            .position(|b| *b == b'>')
            .unwrap_or(region.len());
        let (Some(tag), Some(lower_tag)) = (
            body.get(start..start + end),
            lower_body.get(start..start + end),
        ) else {
            continue;
        };
        let Some(equiv) = attribute_value(tag, lower_tag, b"http-equiv") else {
            continue;
        };
        if !trim_value(equiv).eq_ignore_ascii_case(b"refresh") {
            continue;
        }
        let Some(content) = attribute_value(tag, lower_tag, b"content") else {
            continue;
        };
        let lower_content = content.to_ascii_lowercase();
        let value = match find(&lower_content, b"url", 0) {
            Some(i) => {
                let mut j = i + 3;
                while lower_content.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j += 1;
                }
                if lower_content.get(j) == Some(&b'=') {
                    trim_value(content.get(j + 1..).unwrap_or(&[]))
                } else {
                    content
                }
            }
            None => content,
        };
        out.push(Hint {
            kind: "meta_refresh".into(),
            value: String::from_utf8_lossy(value).into_owned(),
        });
        found += 1;
    }
}

fn script_hints(body: &[u8], out: &mut Vec<Hint>) {
    const NEEDLES: [(&[u8], &str); 3] = [
        (b"window.location", "script_window_location"),
        (b"location.replace(", "script_location_replace"),
        (b"location.href", "script_location_href"),
    ];
    let mut found = 0;
    let mut i = 0;
    let mut skip_until = 0;
    while found < MAX_SCRIPT_HINTS && i < body.len() {
        let rest = body.get(i..).unwrap_or(&[]);
        let hit = if i < skip_until {
            None
        } else {
            NEEDLES.iter().find(|(n, _)| rest.starts_with(n))
        };
        if let Some((needle, kind)) = hit {
            let ctx_end = (i + needle.len() + SCRIPT_CONTEXT).min(body.len());
            let value = String::from_utf8_lossy(body.get(i..ctx_end).unwrap_or(&[])).into_owned();
            out.push(Hint {
                kind: (*kind).into(),
                value,
            });
            found += 1;
            skip_until = i + needle.len();
        }
        i += 1;
    }
}

pub fn scan(body: &[u8]) -> Vec<Hint> {
    let mut out = Vec::new();
    let lower = body.to_ascii_lowercase();
    meta_refresh(body, &lower, &mut out);
    script_hints(body, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(h: &[Hint]) -> Vec<&str> {
        h.iter().map(|h| h.kind.as_str()).collect()
    }

    #[test]
    fn meta_refresh_variants() {
        let h = scan(br#"<html><HEAD><META HTTP-EQUIV="Refresh" CONTENT="5; URL=https://evil.test/a?b=1"></head>"#);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].kind, "meta_refresh");
        assert_eq!(h[0].value, "https://evil.test/a?b=1");

        let h = scan(b"<meta content='0;url = /next' http-equiv=refresh>");
        assert_eq!(h[0].value, "/next");

        let h = scan(b"<meta http-equiv=\"refresh\" content=\"30\">");
        assert_eq!(h[0].value, "30");

        assert!(scan(b"<meta charset=utf-8><meta name=refresh content='0;url=x'>").is_empty());
        assert!(scan(b"<meta http-equiv=\"content-type\" content=\"text/html\">").is_empty());
    }

    #[test]
    fn meta_refresh_is_bounded() {
        let mut page = Vec::new();
        for i in 0..20 {
            page.extend_from_slice(
                format!("<meta http-equiv=refresh content=\"0;url=/p{i}\">").as_bytes(),
            );
        }
        let h = scan(&page);
        assert_eq!(h.len(), MAX_META_HINTS);
        let mut unterminated = b"<meta http-equiv=\"refresh\" content=\"0;url=/x".to_vec();
        unterminated.extend(std::iter::repeat_n(b'a', 10_000));
        assert_eq!(scan(&unterminated).len(), 1);
    }

    #[test]
    fn script_patterns() {
        let h = scan(b"<script>if(x){window.location = 'http://a.test/'; } location.replace('/b');top.location.href='/c'</script>");
        assert_eq!(
            kinds(&h),
            [
                "script_window_location",
                "script_location_replace",
                "script_location_href"
            ]
        );
        assert!(h[0].value.starts_with("window.location = 'http://a.test/'"));
    }

    #[test]
    fn window_location_href_is_one_hint() {
        let h = scan(b"window.location.href='/x'");
        assert_eq!(kinds(&h), ["script_window_location"]);
        assert!(h[0].value.contains("href='/x'"));
    }

    #[test]
    fn script_hints_are_capped_and_context_limited() {
        let mut page = Vec::new();
        for _ in 0..50 {
            page.extend_from_slice(b"location.href='/x';");
            page.extend(std::iter::repeat_n(b'z', 500));
        }
        let h = scan(&page);
        assert_eq!(h.len(), MAX_SCRIPT_HINTS);
        assert_eq!(h[0].value.len(), "location.href".len() + SCRIPT_CONTEXT);
    }

    #[test]
    fn binary_and_empty_input() {
        assert!(scan(b"").is_empty());
        let junk: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let _ = scan(&junk);
        let _ = scan(b"<meta");
        let _ = scan(b"<meta http-equiv=");
        let _ = scan(b"<meta http-equiv=\"refresh\" content=");
        let _ = scan(b"location.href");
    }
}
