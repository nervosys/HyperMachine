//! Volumes on a host that is not Linux: every route answers 501, and a
//! sandbox that asks for a mount is refused before anything boots.
//!
//! A volume is a host directory served to the guest by `ninep`, which
//! resolves every path with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`
//! and `O_PATH` descriptors. Those are what keep a guest inside its volume,
//! and only Linux has them; a weaker resolver would be a way out of the
//! volume, not a port. This module exists so the daemon still builds and
//! serves everything else on Windows and macOS.

use std::sync::Arc;

use axum::response::Response;

use hv2_agent::AgentVM;
use hv2_cluster::model::VolumeMount;

use super::{api_error, AppState, StatusCode};

const UNSUPPORTED: &str =
    "volumes need a Linux host: they are served with openat2, which only Linux has";

fn refused() -> Response {
    api_error(StatusCode::NOT_IMPLEMENTED, UNSUPPORTED.to_string())
}

/// Any mount at all is refused, with the reason.
pub(crate) fn check(
    _state: &AppState,
    _team: Option<&hv2_cluster::ownership::TeamId>,
    mounts: &[VolumeMount],
) -> Result<(), String> {
    if mounts.is_empty() {
        Ok(())
    } else {
        Err(UNSUPPORTED.to_string())
    }
}

/// Unreachable in practice, since [`check`] refuses first; refuses anyway.
pub(crate) async fn mount(
    _state: &AppState,
    _team: Option<&hv2_cluster::ownership::TeamId>,
    _vm: &Arc<AgentVM>,
    mounts: &[VolumeMount],
) -> Result<(), String> {
    check(_state, None, mounts)
}

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
pub(crate) async fn read_file() -> Response {
    refused()
}
pub(crate) async fn write_file() -> Response {
    refused()
}
pub(crate) async fn list_dir() -> Response {
    refused()
}
pub(crate) async fn make_dir() -> Response {
    refused()
}
pub(crate) async fn stat() -> Response {
    refused()
}
pub(crate) async fn update() -> Response {
    refused()
}
pub(crate) async fn remove_path() -> Response {
    refused()
}
