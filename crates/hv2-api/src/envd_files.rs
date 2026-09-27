//! envd's file routes: `GET /files` and `POST /files`, and `GET /health`.
//!
//! What `sandbox.files.read` and `sandbox.files.write` call -- plain HTTP on
//! the same port as envd's RPC services, not RPC. A read is
//! `GET /files?path=P`, answered with the file's bytes; a write is
//! `POST /files`, either `multipart/form-data` with one part per file (each
//! part's filename its path, or `?path=` for a single one) or a raw
//! `application/octet-stream` body with `?path=`. Both answer with a JSON
//! list of `{name, type, path}`, one per file written.
//!
//! The bytes move through the guest agent's file operations in chunks, not
//! through a shell: a file of any content, binary included, arrives as it
//! was sent.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde_json::json;

use hv2_agent::AgentVM;

use crate::connect::ConnectBody;

/// The most a single request moves, either way. Files pass through host
/// memory whole; a limit keeps one request from taking it all.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(120);

fn body(bytes: impl Into<Bytes>) -> ConnectBody {
    http_body_util::Full::new(bytes.into())
        .map_err(|never| match never {})
        .boxed_unsync()
}

/// An error in envd's shape: `{"code", "message"}`, which the SDK maps to
/// its exceptions by status.
fn error(status: StatusCode, message: impl std::fmt::Display) -> Response<ConnectBody> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(body(
            json!({ "code": status.as_u16(), "message": message.to_string() }).to_string(),
        ))
        .expect("a static response builds")
}

fn query(request: &Request<Incoming>, key: &str) -> Option<String> {
    let query = request.uri().query()?;
    form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// A path the SDK sends may be relative to the user's home; the only user
/// here is root.
fn absolute(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/root/{path}")
    }
}

/// `GET /health`: envd is up. The SDK asks when a call fails, to tell a
/// dead sandbox from a failed call.
pub fn health() -> Response<ConnectBody> {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(body(Bytes::new()))
        .expect("a static response builds")
}

/// Serve `/files`.
pub async fn handle(vm: Arc<AgentVM>, request: Request<Incoming>) -> Response<ConnectBody> {
    match *request.method() {
        hyper::Method::GET => read(vm, request).await,
        hyper::Method::POST => write(vm, request).await,
        _ => error(StatusCode::METHOD_NOT_ALLOWED, "GET or POST"),
    }
}

async fn read(vm: Arc<AgentVM>, request: Request<Incoming>) -> Response<ConnectBody> {
    let Some(path) = query(&request, "path") else {
        return error(StatusCode::BAD_REQUEST, "path is required");
    };
    let path = absolute(&path);
    match vm.read_file_in_guest(&path, MAX_FILE_BYTES, TIMEOUT).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "application/octet-stream")
            .header(hyper::header::CONTENT_LENGTH, bytes.len())
            .body(body(bytes))
            .expect("a static response builds"),
        Err(e) => {
            let message = e.to_string();
            let status = if message.contains("No such file") {
                StatusCode::NOT_FOUND
            } else if message.contains("is a directory") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            error(status, message)
        }
    }
}

async fn write(vm: Arc<AgentVM>, request: Request<Incoming>) -> Response<ConnectBody> {
    let content_type = request
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if request
        .headers()
        .get(hyper::header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "a compressed upload is not supported; send it uncompressed",
        );
    }
    let single = query(&request, "path");
    let collected = match http_body_util::Limited::new(request.into_body(), MAX_FILE_BYTES as usize)
        .collect()
        .await
    {
        Ok(collected) => collected.to_bytes(),
        Err(e) => return error(StatusCode::PAYLOAD_TOO_LARGE, e),
    };

    let files: Vec<(String, Bytes)> = if content_type.starts_with("multipart/form-data") {
        let Some(boundary) = content_type
            .split(';')
            .find_map(|p| p.trim().strip_prefix("boundary="))
            .map(|b| b.trim_matches('"').to_string())
        else {
            return error(StatusCode::BAD_REQUEST, "multipart without a boundary");
        };
        match parse_multipart(&collected, &boundary) {
            Ok(parts) => parts
                .into_iter()
                .map(|(filename, data)| {
                    // A single file may be named by `?path=` instead.
                    let path = match (&single, filename) {
                        (Some(path), _) => path.clone(),
                        (None, Some(name)) => name,
                        (None, None) => String::new(),
                    };
                    (path, data)
                })
                .collect(),
            Err(e) => return error(StatusCode::BAD_REQUEST, e),
        }
    } else {
        match single {
            Some(path) => vec![(path, collected)],
            None => return error(StatusCode::BAD_REQUEST, "path is required"),
        }
    };

    let mut written = Vec::with_capacity(files.len());
    for (path, data) in files {
        if path.is_empty() {
            return error(StatusCode::BAD_REQUEST, "a file without a path");
        }
        let path = absolute(&path);
        if let Err(e) = vm.write_file_in_guest(&path, data.to_vec(), TIMEOUT).await {
            return error(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        written.push(json!({ "name": name, "type": "file", "path": path }));
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(body(serde_json::Value::Array(written).to_string()))
        .expect("a static response builds")
}

/// The file parts of a `multipart/form-data` body: each part's filename, if
/// it has one, and its bytes.
fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<(Option<String>, Bytes)>, String> {
    let delimiter = format!("--{boundary}");
    let delimiter = delimiter.as_bytes();
    let find = |haystack: &[u8], needle: &[u8], from: usize| {
        haystack[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|at| at + from)
    };
    let mut parts = Vec::new();
    let mut at = find(body, delimiter, 0).ok_or("no multipart boundary in the body")?;
    loop {
        at += delimiter.len();
        if body[at..].starts_with(b"--") {
            return Ok(parts);
        }
        let head_start = at + 2; // CRLF after the delimiter
        let head_end = find(body, b"\r\n\r\n", head_start).ok_or("a part without headers")?;
        let head = String::from_utf8_lossy(&body[head_start..head_end]);
        let data_start = head_end + 4;
        let next = find(body, delimiter, data_start).ok_or("an unterminated part")?;
        // The CRLF before the next delimiter belongs to the delimiter.
        let data_end = next.saturating_sub(2).max(data_start);
        let filename = head.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            if !lower.starts_with("content-disposition") {
                return None;
            }
            let at = lower.find("filename=\"")? + "filename=\"".len();
            let rest = &line[at..];
            Some(rest[..rest.find('"')?].to_string())
        });
        parts.push((
            filename,
            Bytes::copy_from_slice(&body[data_start..data_end]),
        ));
        at = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_parts_come_out_with_their_filenames_and_exact_bytes() {
        let binary = [0u8, 13, 10, 255, 45, 45];
        let mut body = Vec::new();
        body.extend_from_slice(b"--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"/tmp/a.py\"\r\nContent-Type: application/octet-stream\r\n\r\nprint(1)\r\n");
        body.extend_from_slice(
            b"--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"b.bin\"\r\n\r\n",
        );
        body.extend_from_slice(&binary);
        body.extend_from_slice(b"\r\n--XyZ--\r\n");
        let parts = parse_multipart(&body, "XyZ").expect("parses");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0.as_deref(), Some("/tmp/a.py"));
        assert_eq!(&parts[0].1[..], b"print(1)");
        assert_eq!(parts[1].0.as_deref(), Some("b.bin"));
        assert_eq!(&parts[1].1[..], &binary);
    }

    #[test]
    fn relative_paths_are_under_the_home_directory() {
        assert_eq!(absolute("x/y.txt"), "/root/x/y.txt");
        assert_eq!(absolute("/etc/hosts"), "/etc/hosts");
    }
}
