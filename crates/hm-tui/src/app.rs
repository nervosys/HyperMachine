//! Application state and the background polling/action tasks that feed it.
//!
//! Nothing here touches the terminal; [`crate::ui`] does that. This module
//! only knows about the two backends in [`crate::api`] and a channel of
//! [`Event`]s the UI thread drains each tick.

use crate::api::{SandboxApiClient, SandboxInfo, VmApiClient, VmInfo};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// Which list is on screen. `Tab::next`/`prev` wrap, so one key cycles
/// through both without a special case at either end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Vms,
    Sandboxes,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Self::Vms => Self::Sandboxes,
            Self::Sandboxes => Self::Vms,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Vms => "VMs",
            Self::Sandboxes => "Sandboxes",
        }
    }
}

/// What a background task hands back to the UI thread. Each refresh
/// replaces its whole list rather than patching it: the lists are small and
/// a full replace can't drift from what the backend actually has.
pub enum Event {
    Vms(anyhow::Result<Vec<VmInfo>>),
    Sandboxes(anyhow::Result<Vec<SandboxInfo>>),
    /// An action's own result (start/stop/delete), reported separately from
    /// a refresh so the status line can say what just happened.
    ActionDone {
        label: String,
        result: anyhow::Result<()>,
    },
}

pub struct App {
    pub tab: Tab,
    pub vms: Vec<VmInfo>,
    pub sandboxes: Vec<SandboxInfo>,
    pub selected: usize,
    pub status: String,
    pub vm_api: VmApiClient,
    pub sandbox_api: SandboxApiClient,
    pub last_refresh: Instant,
    pub refresh_every: Duration,
    pub confirm_delete: Option<String>,
    pub should_quit: bool,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    runtime: tokio::runtime::Handle,
}

impl App {
    pub fn new(
        vm_api_url: String,
        sandbox_api_url: String,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let app = Self {
            tab: Tab::Vms,
            vms: Vec::new(),
            sandboxes: Vec::new(),
            selected: 0,
            status: format!("connecting to {vm_api_url} and {sandbox_api_url} ..."),
            vm_api: VmApiClient::new(&vm_api_url),
            sandbox_api: SandboxApiClient::new(&sandbox_api_url),
            last_refresh: Instant::now() - Duration::from_secs(3600),
            refresh_every: Duration::from_secs(2),
            confirm_delete: None,
            should_quit: false,
            events_tx,
            events_rx,
            runtime,
        };
        app.refresh();
        app
    }

    /// Current tab's row count, so the UI and the key handler agree on how
    /// far `selected` may move without either guessing at the other's list.
    pub fn row_count(&self) -> usize {
        match self.tab {
            Tab::Vms => self.vms.len(),
            Tab::Sandboxes => self.sandboxes.len(),
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.row_count();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, len as isize - 1) as usize;
    }

    pub fn switch_tab(&mut self) {
        self.tab = self.tab.next();
        self.selected = 0;
        self.confirm_delete = None;
    }

    /// Kick off a refresh of both lists if the interval has passed, or
    /// unconditionally when `force` is set (the `r` key).
    pub fn maybe_refresh(&mut self) {
        if self.last_refresh.elapsed() >= self.refresh_every {
            self.refresh();
        }
    }

    pub fn refresh(&self) {
        let api = self.vm_api.clone();
        let tx = self.events_tx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(Event::Vms(api.list().await));
        });

        let api = self.sandbox_api.clone();
        let tx = self.events_tx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(Event::Sandboxes(api.list().await));
        });
    }

    /// Drain every pending event without blocking; called once per UI tick.
    pub fn drain_events(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                Event::Vms(Ok(vms)) => {
                    self.vms = vms;
                    self.last_refresh = Instant::now();
                }
                Event::Vms(Err(e)) => {
                    self.last_refresh = Instant::now();
                    self.status = format!("VM API: {e}");
                }
                Event::Sandboxes(Ok(sandboxes)) => {
                    self.sandboxes = sandboxes;
                }
                Event::Sandboxes(Err(e)) => {
                    self.status = format!("sandbox API: {e}");
                }
                Event::ActionDone { label, result } => {
                    self.status = match result {
                        Ok(()) => format!("{label}: done"),
                        Err(e) => format!("{label}: {e}"),
                    };
                    self.refresh();
                }
            }
        }
    }

    fn run_action(
        &self,
        label: String,
        fut: impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        let tx = self.events_tx.clone();
        self.runtime.spawn(async move {
            let result = fut.await;
            let _ = tx.send(Event::ActionDone { label, result });
        });
    }

    /// Start or stop the selected VM, toggling on its last-known state.
    pub fn toggle_selected_vm(&mut self) {
        let Some(vm) = self.vms.get(self.selected).cloned() else {
            return;
        };
        let api = self.vm_api.clone();
        if vm.state == crate::api::VmState::Running {
            self.status = format!("stopping {} ...", vm.name);
            self.run_action(format!("stop {}", vm.name), async move {
                api.stop(&vm.name).await
            });
        } else {
            self.status = format!("starting {} ...", vm.name);
            self.run_action(format!("start {}", vm.name), async move {
                api.start(&vm.name).await
            });
        }
    }

    /// Arm or carry out a delete: the first `d` asks for confirmation, a
    /// second `d` on the same row within the same press carries it out, so
    /// one stray keystroke can't destroy a VM or sandbox outright.
    pub fn request_delete_selected(&mut self) {
        match self.tab {
            Tab::Vms => {
                let Some(vm) = self.vms.get(self.selected).cloned() else {
                    return;
                };
                if self.confirm_delete.as_deref() == Some(vm.name.as_str()) {
                    self.confirm_delete = None;
                    let api = self.vm_api.clone();
                    self.status = format!("deleting {} ...", vm.name);
                    self.run_action(format!("delete {}", vm.name), async move {
                        api.delete(&vm.name).await
                    });
                } else {
                    self.confirm_delete = Some(vm.name.clone());
                    self.status = format!("press d again to delete {}", vm.name);
                }
            }
            Tab::Sandboxes => {
                let Some(sb) = self.sandboxes.get(self.selected).cloned() else {
                    return;
                };
                if self.confirm_delete.as_deref() == Some(sb.sandbox_id.as_str()) {
                    self.confirm_delete = None;
                    let api = self.sandbox_api.clone();
                    self.status = format!("deleting {} ...", sb.sandbox_id);
                    self.run_action(format!("delete {}", sb.sandbox_id), async move {
                        api.delete(&sb.sandbox_id).await
                    });
                } else {
                    self.confirm_delete = Some(sb.sandbox_id.clone());
                    self.status = format!("press d again to delete {}", sb.sandbox_id);
                }
            }
        }
    }
}
