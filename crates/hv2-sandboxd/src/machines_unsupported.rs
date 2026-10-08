//! Machines on a host that is not Linux: every route answers 501.
//!
//! A machine's root disk is made with `mkfs.ext4` from a tree with Unix file
//! modes and symlinks, and KVM is the backend that runs it. This module
//! exists so the daemon still builds and serves everything else elsewhere.

use std::sync::Arc;

use axum::response::Response;

use super::{api_error, AppState, StatusCode};

fn refused() -> Response {
    api_error(
        StatusCode::NOT_IMPLEMENTED,
        "machines need a Linux host with KVM".to_string(),
    )
}

pub(crate) async fn supervise(_state: Arc<AppState>) {}

pub(crate) async fn create() -> Response {
    refused()
}
pub(crate) async fn list() -> Response {
    refused()
}
pub(crate) async fn get() -> Response {
    refused()
}
pub(crate) async fn delete() -> Response {
    refused()
}
pub(crate) async fn action() -> Response {
    refused()
}
pub(crate) async fn exec() -> Response {
    refused()
}
pub(crate) async fn console() -> Response {
    refused()
}
