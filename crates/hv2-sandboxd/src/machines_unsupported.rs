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
pub(crate) async fn processes() -> Response {
    refused()
}
pub(crate) async fn reverse_tunnel() -> Response {
    refused()
}
pub(crate) async fn console() -> Response {
    refused()
}
pub(crate) async fn network_decisions() -> Response {
    refused()
}
pub(crate) async fn console_input() -> Response {
    refused()
}

/// A machine's runs: the same answer.
pub(crate) mod runs {
    use super::{refused, Response};

    pub(crate) async fn start() -> Response {
        refused()
    }
    pub(crate) async fn list() -> Response {
        refused()
    }
    pub(crate) async fn get() -> Response {
        refused()
    }
    pub(crate) async fn stdin() -> Response {
        refused()
    }
    pub(crate) async fn signal() -> Response {
        refused()
    }
}

/// The files in a machine's snapshots: the same answer.
pub(crate) mod snapshot_files {
    use super::{refused, Response};

    pub(crate) async fn list() -> Response {
        refused()
    }
    pub(crate) async fn file() -> Response {
        refused()
    }
}

/// A machine's snapshots: the same answer.
pub(crate) mod snapshots {
    use super::{refused, Response};

    pub(crate) async fn take() -> Response {
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
    pub(crate) async fn restore() -> Response {
        refused()
    }
    pub(crate) async fn disk() -> Response {
        refused()
    }
}

/// A machine's published ports: the same answer.
pub(crate) mod ports {
    use super::{refused, Response};

    pub(crate) async fn publish() -> Response {
        refused()
    }
    pub(crate) async fn list() -> Response {
        refused()
    }
    pub(crate) async fn unpublish() -> Response {
        refused()
    }
    /// No machine's port is routed through the proxy on this host.
    pub(crate) fn proxied(
        _state: &crate::AppState,
        _machine_id: &str,
        _port: u16,
    ) -> Option<std::net::SocketAddr> {
        None
    }
}

/// Resizing a machine: the same answer.
pub(crate) mod resize {
    use super::{refused, Response};

    pub(crate) async fn patch() -> Response {
        refused()
    }
}

/// A machine's files: the same answer.
pub(crate) mod files {
    use super::{refused, Response};

    pub(crate) const MAX_FILE: usize = 64 << 20;

    pub(crate) async fn get() -> Response {
        refused()
    }
    pub(crate) async fn put() -> Response {
        refused()
    }
}

/// A machine's prompts: the same answer.
pub(crate) mod prompts {
    use super::{refused, Response};

    pub(crate) async fn start() -> Response {
        refused()
    }
    pub(crate) async fn list() -> Response {
        refused()
    }
    pub(crate) async fn get() -> Response {
        refused()
    }
    pub(crate) async fn interrupt() -> Response {
        refused()
    }
}

/// A machine's conversations: the same answer.
pub(crate) mod conversations {
    use super::{refused, Response};

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
    pub(crate) async fn message() -> Response {
        refused()
    }
    pub(crate) async fn get_transcript() -> Response {
        refused()
    }
    pub(crate) async fn interrupt() -> Response {
        refused()
    }
}

/// A machine's desktop: the same answer.
pub(crate) mod desktop {
    use super::{refused, Response};

    pub(crate) async fn get() -> Response {
        refused()
    }
    pub(crate) async fn screenshot() -> Response {
        refused()
    }
    pub(crate) async fn act() -> Response {
        refused()
    }
}
