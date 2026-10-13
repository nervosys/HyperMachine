//! A machine's desktop, and computer use on it.
//!
//! A machine from an image, created with `guestAgent` and `desktop`, has a
//! desktop installed at its first boot (see `seed::desktop_install`). It is
//! a virtual display in the guest, `:1`, with a window manager and a
//! terminal on it. noVNC serves it to a browser on guest port
//! [`seed::DESKTOP_WEB_PORT`], which the machine's owner publishes as any
//! other port. It is not the emulated display device of a VM: the guest
//! draws it in memory, and nothing on the node renders it.
//!
//! - `GET  /machines/{name}/desktop` says what it is and whether it is up.
//! - `GET  /machines/{name}/desktop/screenshot` is a PNG of it.
//! - `POST /machines/{name}/desktop/actions` `{"action", ...}` moves the
//!   pointer, clicks, drags, types, presses keys or scrolls: the actions of
//!   `guest/hv2-computer.py`, which this runs in the guest with the action
//!   as one argument, never through a shell.
//!
//! Each coding agent installed in the machine is given the same actions as
//! an MCP server named `computer`, so that an agent sees the screen and uses
//! it without going through the node.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{header, HeaderMap};
use serde::Serialize;
use serde_json::{json, Value};

use super::super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State, StatusCode,
};
use super::{named, running, seed, AgentVM};

/// A desktop's size when only `true` is asked for: what computer-use models
/// are most often shown.
pub(crate) const DEFAULT: Size = Size {
    width: 1280,
    height: 800,
};

/// The least and most of each side.
const WIDTHS: std::ops::RangeInclusive<u32> = 640..=3840;
const HEIGHTS: std::ops::RangeInclusive<u32> = 480..=2160;

/// How long one action, or one picture, may take in the guest.
const ACT_TIMEOUT: Duration = Duration::from_secs(60);

/// The most a screenshot may be.
const MAX_SCREENSHOT: u64 = 32 << 20;

/// A desktop's size, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Size {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// What a caller asked for: `true`, or a size.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Asked {
    Yes(bool),
    Sized(Size),
}

impl Asked {
    /// The size to install, or why it cannot be. `false` is refused rather
    /// than taken for no desktop: leaving the field out says that.
    pub(crate) fn size(&self) -> Result<Size, String> {
        let size = match self {
            Asked::Yes(true) => DEFAULT,
            Asked::Yes(false) => {
                return Err("desktop is true or a size; leave it out for none".to_string())
            }
            Asked::Sized(size) => *size,
        };
        if !WIDTHS.contains(&size.width) || !HEIGHTS.contains(&size.height) {
            return Err(format!(
                "a desktop is {} to {} pixels wide and {} to {} high",
                WIDTHS.start(),
                WIDTHS.end(),
                HEIGHTS.start(),
                HEIGHTS.end()
            ));
        }
        if size.width % 2 != 0 || size.height % 2 != 0 {
            return Err("a desktop's width and height are even".to_string());
        }
        Ok(size)
    }
}

/// The machine's desktop and its guest agent, or the reply that says why
/// there is none to use.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn desktop_of(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
) -> Result<(Size, Arc<AgentVM>), Response> {
    let machine = named(state, headers, name)?;
    let Some(size) = machine.desktop else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!(
                "machine {name} has no desktop: create a machine from an image with \
                 guestAgent and desktop"
            ),
        ));
    };
    match running(state, headers, name)? {
        (vm, true) => Ok((size, vm)),
        (_, false) => Err(api_error(
            StatusCode::CONFLICT,
            format!("machine {name} has no guest agent answering yet, and its desktop is used through one"),
        )),
    }
}

/// Run the computer-use helper with `args`: what it printed, or the reply
/// that says why it could not. It exits 2 when what was asked is refused,
/// saying why.
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
async fn computer(vm: &AgentVM, args: &[String]) -> Result<String, Response> {
    let ran = vm
        .exec_in_guest(seed::COMPUTER, args, ACT_TIMEOUT)
        .await
        .map_err(|e| api_error(StatusCode::BAD_GATEWAY, format!("computer use: {e}")))?;
    match ran.exit_code {
        Some(0) => Ok(ran.stdout),
        Some(2) => Err(api_error(StatusCode::BAD_REQUEST, ran.stderr.trim())),
        _ => Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "the desktop did not answer ({}); is it installed yet? See {}",
                ran.stderr.trim(),
                seed::DESKTOP_LOG
            ),
        )),
    }
}

/// `GET /machines/{name}/desktop`.
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let (size, vm) = match desktop_of(&state, &headers, &name) {
        Ok(found) => found,
        Err(response) => return response,
    };
    let ready = vm
        .exec_in_guest(
            "/bin/test",
            &["-e".to_string(), seed::DESKTOP_READY.to_string()],
            ACT_TIMEOUT,
        )
        .await
        .map(|ran| ran.exit_code == Some(0))
        .unwrap_or(false);
    Json(json!({
        "width": size.width,
        "height": size.height,
        "display": ":1",
        "ready": ready,
        "webPort": seed::DESKTOP_WEB_PORT,
        "webPath": "/vnc.html",
        "log": seed::DESKTOP_LOG,
    }))
    .into_response()
}

/// `GET /machines/{name}/desktop/screenshot`: a PNG of the whole screen.
pub(crate) async fn screenshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let (_, vm) = match desktop_of(&state, &headers, &name) {
        Ok(found) => found,
        Err(response) => return response,
    };
    // A name of its own, so that two pictures taken at once do not meet.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = format!("/tmp/hv2-screenshot-{nanos}.png");
    if let Err(response) = computer(&vm, &["screenshot".to_string(), path.clone()]).await {
        return response;
    }
    let read = vm
        .read_file_in_guest(&path, MAX_SCREENSHOT, ACT_TIMEOUT)
        .await;
    // Not left behind, whether it was read or not.
    let _ = vm
        .exec_in_guest("/bin/rm", &["-f".to_string(), path], ACT_TIMEOUT)
        .await;
    match read {
        Ok(png) => ([(header::CONTENT_TYPE, "image/png")], png).into_response(),
        Err(e) => api_error(
            StatusCode::BAD_GATEWAY,
            format!("reading the screenshot: {e}"),
        ),
    }
}

/// `POST /machines/{name}/desktop/actions`: one action, as
/// `guest/hv2-computer.py` takes it. Answers with where the pointer is.
pub(crate) async fn act(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(action): Json<Value>,
) -> Response {
    if !action.is_object() {
        return api_error(StatusCode::BAD_REQUEST, "an action is a JSON object");
    }
    let (_, vm) = match desktop_of(&state, &headers, &name) {
        Ok(found) => found,
        Err(response) => return response,
    };
    match computer(&vm, &["act".to_string(), action.to_string()]).await {
        Ok(out) => match serde_json::from_str::<Value>(out.trim()) {
            Ok(done) => Json(done).into_response(),
            Err(_) => api_error(
                StatusCode::BAD_GATEWAY,
                format!("the desktop answered {:?}", out.trim()),
            ),
        },
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(value: Value) -> Result<Size, String> {
        serde_json::from_value::<Asked>(value)
            .map_err(|e| e.to_string())?
            .size()
    }

    #[test]
    fn a_desktop_is_true_or_a_size_that_a_screen_can_be() {
        assert_eq!(asked(json!(true)), Ok(DEFAULT));
        assert_eq!(
            asked(json!({"width": 1920, "height": 1080})),
            Ok(Size {
                width: 1920,
                height: 1080
            })
        );
        for refused in [
            json!(false),
            json!({"width": 320, "height": 240}),
            json!({"width": 8000, "height": 1080}),
            json!({"width": 1281, "height": 800}),
            json!({"width": 1280}),
            json!("yes"),
        ] {
            assert!(asked(refused.clone()).is_err(), "{refused}");
        }
    }
}
