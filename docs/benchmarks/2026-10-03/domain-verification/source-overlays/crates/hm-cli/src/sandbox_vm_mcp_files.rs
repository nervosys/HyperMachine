//! Bounded guest-file transfers over an operator-selected envd route.
use super::super::Api;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{header, Method};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_ENCODED_BYTES: usize = MAX_FILE_BYTES.div_ceil(3) * 4;

#[derive(Deserialize)]
#[serde(
    tag = "name",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum FileCall {
    FileUpload {
        id: String,
        path: String,
        data_base64: String,
    },
    FileDownload {
        id: String,
        path: String,
    },
}

pub(super) struct Prepared {
    id: String,
    path: String,
    upload: Option<Vec<u8>>,
}

impl FileCall {
    pub(super) fn prepare(self) -> Result<Prepared> {
        let (id, path, upload) = match self {
            Self::FileUpload {
                id,
                path,
                data_base64,
            } => {
                if data_base64.len() > MAX_ENCODED_BYTES {
                    bail!("file exceeds 256 KiB");
                }
                let bytes = STANDARD.decode(data_base64).context("invalid base64")?;
                if bytes.len() > MAX_FILE_BYTES {
                    bail!("file exceeds 256 KiB");
                }
                (id, path, Some(bytes))
            }
            Self::FileDownload { id, path } => (id, path, None),
        };
        if id.trim().is_empty() || matches!(id.as_str(), "." | "..") || id.contains('\0') {
            bail!("invalid sandbox ID");
        }
        if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            bail!("invalid guest path");
        }
        Ok(Prepared { id, path, upload })
    }
}

#[derive(Clone)]
pub(super) struct FileRoute {
    envd: Api,
    domain: Option<String>,
}

fn dns_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.starts_with(|c: char| c.is_ascii_alphanumeric())
                && label.ends_with(|c: char| c.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

impl FileRoute {
    pub(super) fn new(endpoint: &str, domain: Option<String>, deadline: u64) -> Result<Self> {
        Self::with_ca(endpoint, domain, deadline, None)
    }

    pub(super) fn with_ca(
        endpoint: &str,
        domain: Option<String>,
        deadline: u64,
        ca: Option<&std::path::Path>,
    ) -> Result<Self> {
        if domain.as_deref().is_some_and(|value| !dns_name(value)) {
            bail!("invalid envd domain");
        }
        Ok(Self {
            envd: Api::with_ca(endpoint, deadline, None, ca)?,
            domain,
        })
    }

    pub(super) async fn execute(&self, api: &Api, operation: Prepared) -> Result<Value> {
        let mut url = self.envd.base.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid envd endpoint"))?
            .pop_if_empty()
            .push("files");
        url.query_pairs_mut().append_pair("path", &operation.path);
        let descriptor = api
            .request(
                Method::POST,
                &["sandboxes", &operation.id, "connect"],
                Some(json!({"timeout":300})),
            )
            .await?;
        let token = descriptor["envdAccessToken"]
            .as_str()
            .filter(|value| !value.is_empty())
            .context("missing sandbox credential")?;
        let mut token =
            header::HeaderValue::from_str(token).context("invalid sandbox credential")?;
        token.set_sensitive(true);
        let mut request = match &operation.upload {
            Some(bytes) => self
                .envd
                .client
                .post(url)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes.clone()),
            None => self.envd.client.get(url),
        };
        if let Some(domain) = &self.domain {
            let label = descriptor["envdHost"]
                .as_str()
                .context("missing envd host label")?;
            if !dns_name(label) || label.contains('.') {
                bail!("invalid envd host label");
            }
            let host = format!("{label}.{domain}");
            if !dns_name(&host) {
                bail!("invalid envd host");
            }
            request = request.header(header::HOST, host);
        }
        // The platform key stays on `api`; envd receives only its sandbox token.
        let mut response = request
            .header("x-access-token", token)
            .send()
            .await?
            .error_for_status()?;
        if let Some(bytes) = operation.upload {
            return Ok(json!({"path":operation.path,"bytes":bytes.len()}));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_FILE_BYTES as u64)
        {
            bail!("file exceeds 256 KiB");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_FILE_BYTES {
                bail!("file exceeds 256 KiB");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(json!({"path":operation.path,"bytes":bytes.len(),"data_base64":STANDARD.encode(bytes)}))
    }
}

pub(super) fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1});
    let path = json!({"type":"string","minLength":1,"maxLength":4096});
    vec![
        json!({"name":"file_upload","description":"Write or replace a guest file from base64 (maximum 256 KiB)","inputSchema":{"type":"object","properties":{"id":string,"path":path,"data_base64":{"type":"string","maxLength":MAX_ENCODED_BYTES}},"required":["id","path","data_base64"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"file_download","description":"Read a guest file as base64 (maximum 256 KiB); connecting may resume the sandbox","inputSchema":{"type":"object","properties":{"id":string,"path":path},"required":["id","path"],"additionalProperties":false},"annotations":{"readOnlyHint":false}}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Bytes, extract::Query, http::HeaderMap, routing::post, Json, Router};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[test]
    fn invalid_arguments_cannot_select_hosts_or_exceed_bounds() {
        for arguments in [
            json!({"id":"vm","path":"/file","endpoint":"http://attacker"}),
            json!({"id":"vm","path":"/file","destination":"/host/file"}),
        ] {
            assert!(serde_json::from_value::<FileCall>(
                json!({"name":"file_download","arguments":arguments})
            )
            .is_err());
        }
        for data in ["!".into(), STANDARD.encode(vec![0; MAX_FILE_BYTES + 1])] {
            assert!(FileCall::FileUpload {
                id: "vm".into(),
                path: "/file".into(),
                data_base64: data
            }
            .prepare()
            .is_err());
        }
        for path in [String::new(), "a\0b".into(), "x".repeat(4097)] {
            assert!(FileCall::FileDownload {
                id: "vm".into(),
                path
            }
            .prepare()
            .is_err());
        }
        assert!(FileRoute::new("http://user:secret@localhost", None, 1).is_err());
        assert!(FileRoute::new("http://localhost", Some("bad\r\nhost".into()), 1).is_err());
    }

    async fn server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (endpoint, task)
    }

    async fn platform() -> (Api, tokio::task::JoinHandle<()>) {
        let (endpoint, task) = server(Router::new().route(
            "/sandboxes/vm/connect",
            post(|headers: HeaderMap| async move {
                assert_eq!(headers["x-api-key"], "platform-secret");
                assert!(!headers.contains_key("x-access-token"));
                Json(json!({"envdAccessToken":"sandbox-secret","envdHost":"49983-vm"}))
            }),
        ))
        .await;
        (
            Api::new(&endpoint, 5, Some("platform-secret".into())).unwrap(),
            task,
        )
    }

    fn envd_headers(headers: &HeaderMap) {
        assert_eq!(headers["x-access-token"], "sandbox-secret");
        assert_eq!(headers["host"], "49983-vm.sandbox.local");
        assert!(!headers.contains_key("x-api-key"));
    }

    #[tokio::test]
    async fn binary_roundtrip_keeps_credentials_and_query_paths_separate() {
        let (api, platform_task) = platform().await;
        let stored = Arc::new(Mutex::new(Vec::<u8>::new()));
        let upload = Arc::clone(&stored);
        let download = Arc::clone(&stored);
        let path = "/root/file'&query=literal.bin";
        let (endpoint, envd_task) = server(
            Router::new().route(
                "/files",
                post(
                    move |headers: HeaderMap,
                          Query(query): Query<HashMap<String, String>>,
                          body: Bytes| {
                        let upload = Arc::clone(&upload);
                        async move {
                            envd_headers(&headers);
                            assert_eq!(query.get("path").unwrap(), path);
                            assert_eq!(query.len(), 1);
                            *upload.lock().unwrap() = body.to_vec();
                            Json(json!({"ok":true}))
                        }
                    },
                )
                .get(
                    move |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| {
                        let download = Arc::clone(&download);
                        async move {
                            envd_headers(&headers);
                            assert_eq!(query.get("path").unwrap(), path);
                            download.lock().unwrap().clone()
                        }
                    },
                ),
            ),
        )
        .await;
        let route = FileRoute::new(&endpoint, Some("sandbox.local".into()), 5).unwrap();
        let bytes: Vec<u8> = (0..=255).cycle().take(MAX_FILE_BYTES).collect();
        let upload = FileCall::FileUpload {
            id: "vm".into(),
            path: path.into(),
            data_base64: STANDARD.encode(&bytes),
        }
        .prepare()
        .unwrap();
        assert_eq!(
            route.execute(&api, upload).await.unwrap()["bytes"],
            MAX_FILE_BYTES
        );
        let download = FileCall::FileDownload {
            id: "vm".into(),
            path: path.into(),
        }
        .prepare()
        .unwrap();
        let value = route.execute(&api, download).await.unwrap();
        assert_eq!(
            STANDARD
                .decode(value["data_base64"].as_str().unwrap())
                .unwrap(),
            bytes
        );
        assert!(value.get("envdAccessToken").is_none());
        let frame = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}});
        assert!(serde_json::to_vec(&frame).unwrap().len() < 1024 * 1024);
        platform_task.abort();
        envd_task.abort();
    }

    #[tokio::test]
    async fn oversized_chunked_download_is_rejected() {
        let (api, platform_task) = platform().await;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let envd_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
                assert!(request.len() < 8192);
            }
            // Unknown-length HTTP body exercises the streaming size guard.
            let header = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", MAX_FILE_BYTES + 1);
            stream.write_all(header.as_bytes()).await.unwrap();
            stream
                .write_all(&vec![0; MAX_FILE_BYTES + 1])
                .await
                .unwrap();
            let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
        });
        let route = FileRoute::new(&endpoint, None, 5).unwrap();
        let operation = FileCall::FileDownload {
            id: "vm".into(),
            path: "/file".into(),
        }
        .prepare()
        .unwrap();
        let error = route.execute(&api, operation).await.unwrap_err();
        assert!(error.to_string().contains("256 KiB"), "{error:#}");
        platform_task.abort();
        envd_task.abort();
    }
}
