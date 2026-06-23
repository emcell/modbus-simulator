//! Transport supervisor: starts, stops, and hot-reconfigures the TCP +
//! RTU listeners when the active context's transport settings change.
//!
//! All mutations that touch transport config (`configureTcp`,
//! `configureRtu`, `switchContext`, `deleteContext` if it was active) call
//! [`AppState::reconfigure_transports`]. The supervisor aborts the
//! currently-running listener tasks and spawns fresh ones for the new
//! config — no process restart required.
//!
//! Bind + serial-open happen synchronously inside `reconfigure` so that
//! errors (port in use, device missing, …) surface immediately and can be
//! shown to the user in the UI, rather than ending up only in the log.

use std::sync::Arc;

use modsim_core::model::{RtuTransport, TcpTransport};
use parking_lot::Mutex;
use tokio::task::AbortHandle;

use crate::state::AppState;
use crate::transport::{rtu as modbus_rtu, tcp as modbus_tcp};

struct TcpRunning {
    handle: AbortHandle,
    cfg: TcpTransport,
}

struct RtuRunning {
    handle: AbortHandle,
    /// Set if this task took ownership of a virtual-serial master fd; used
    /// on stop to remove the now-dead registry entry.
    vs_id: Option<String>,
    cfg: RtuTransport,
}

/// Snapshot of each protocol's current runtime state, for display in the UI.
#[derive(Clone, Debug, Default)]
pub struct TransportStatus {
    pub tcp: TransportState,
    pub rtu: TransportState,
}

#[derive(Clone, Debug, Default)]
pub enum TransportState {
    /// No transport is configured / enabled for this protocol.
    #[default]
    Disabled,
    /// Running and listening. `description` is human-readable, e.g.
    /// `"127.0.0.1:502"` or `"/tmp/modsim-a"`.
    Running { description: String },
    /// Configuration is enabled but could not be applied.
    Error {
        description: String,
        message: String,
    },
}

impl TransportState {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

pub struct TransportSupervisor {
    tcp: Mutex<Option<TcpRunning>>,
    rtu: Mutex<Option<RtuRunning>>,
    status: Mutex<TransportStatus>,
}

impl Default for TransportSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportSupervisor {
    pub fn new() -> Self {
        Self {
            tcp: Mutex::new(None),
            rtu: Mutex::new(None),
            status: Mutex::new(TransportStatus::default()),
        }
    }

    pub fn snapshot(&self) -> TransportStatus {
        self.status.lock().clone()
    }

    pub fn stop_all(&self, state: &Arc<AppState>) {
        if let Some(r) = self.tcp.lock().take() {
            r.handle.abort();
        }
        if let Some(r) = self.rtu.lock().take() {
            r.handle.abort();
            if let Some(id) = r.vs_id {
                recycle_pty(state, &id);
            }
        }
        *self.status.lock() = TransportStatus::default();
    }

    /// Tear down and rebuild the PTY pair for `id`, re-pointing its symlink
    /// and re-binding RTU if it was using this virtual serial. Lets the user
    /// recover a dead PTY (dangling symlink) without restarting the process.
    #[cfg(unix)]
    pub async fn recreate_virtual_serial(
        &self,
        state: &Arc<AppState>,
        id: &str,
    ) -> anyhow::Result<()> {
        // If the RTU task currently owns this vs, stop it first so the master
        // fd is released before we rebuild the pair.
        let was_bound = {
            let mut g = self.rtu.lock();
            if g.as_ref().and_then(|r| r.vs_id.as_deref()) == Some(id) {
                g.take().unwrap().handle.abort();
                self.status.lock().rtu = TransportState::Disabled;
                true
            } else {
                false
            }
        };
        rebuild_pty(state, id)?;
        // If RTU had been bound to it, re-apply so it re-takes the fresh
        // master (the rtu slot is now empty, so the idempotency check in
        // `reconfigure_rtu` won't short-circuit).
        if was_bound {
            self.reconfigure(state).await;
        }
        Ok(())
    }

    /// Apply the current active context's transport settings. Idempotent:
    /// if the config matches what's already running, the listener task
    /// keeps running untouched (crucial for RTU + virtual-serial, where a
    /// needless restart would destroy the user's PTY).
    pub async fn reconfigure(&self, state: &Arc<AppState>) {
        let ctx = state.world.read().active_context().cloned();

        let want_tcp = ctx
            .as_ref()
            .filter(|c| c.transport.tcp.enabled)
            .map(|c| c.transport.tcp.clone());
        let want_rtu = ctx
            .as_ref()
            .filter(|c| c.transport.rtu.enabled)
            .map(|c| c.transport.rtu.clone());

        self.reconfigure_tcp(state, want_tcp).await;
        self.reconfigure_rtu(state, want_rtu).await;
    }

    async fn reconfigure_tcp(&self, state: &Arc<AppState>, want: Option<TcpTransport>) {
        let current = self.tcp.lock().as_ref().map(|r| r.cfg.clone());
        if current == want {
            return; // unchanged
        }
        if let Some(r) = self.tcp.lock().take() {
            r.handle.abort();
        }
        let Some(cfg) = want else {
            self.status.lock().tcp = TransportState::Disabled;
            return;
        };
        let bind = if cfg.bind.is_empty() {
            "0.0.0.0".to_string()
        } else {
            cfg.bind.clone()
        };
        let port = if cfg.port == 0 { 502 } else { cfg.port };
        let description = format!("{bind}:{port}");
        match modbus_tcp::bind_listener(&bind, port).await {
            Ok(listener) => {
                let st = state.clone();
                let desc = description.clone();
                let handle = tokio::spawn(async move {
                    if let Err(e) = modbus_tcp::serve_listener(st, listener).await {
                        tracing::error!("modbus tcp serve: {e}");
                    }
                });
                *self.tcp.lock() = Some(TcpRunning {
                    handle: handle.abort_handle(),
                    cfg,
                });
                self.status.lock().tcp = TransportState::Running { description: desc };
            }
            Err(e) => {
                tracing::error!("modbus tcp bind {description} failed: {e}");
                self.status.lock().tcp = TransportState::Error {
                    description,
                    message: e.to_string(),
                };
            }
        }
    }

    async fn reconfigure_rtu(&self, state: &Arc<AppState>, want: Option<RtuTransport>) {
        let current = self.rtu.lock().as_ref().map(|r| r.cfg.clone());
        if current == want {
            return;
        }
        if let Some(r) = self.rtu.lock().take() {
            r.handle.abort();
            if let Some(id) = r.vs_id {
                recycle_pty(state, &id);
            }
        }
        let Some(cfg) = want else {
            self.status.lock().rtu = TransportState::Disabled;
            return;
        };

        // ---- Virtual serial path (unix only) -----------------------------
        #[cfg(unix)]
        {
            if let Some(vs_id) = cfg.virtual_serial_id.as_ref() {
                let description = format!("virtual serial {vs_id}");
                let Some(master_fd) = state.ptys.take_master(vs_id) else {
                    let msg = format!("virtual serial '{vs_id}' not found");
                    tracing::warn!(msg);
                    self.status.lock().rtu = TransportState::Error {
                        description,
                        message: msg,
                    };
                    return;
                };
                match crate::transport::ptystream::PtyStream::new(master_fd) {
                    Ok(stream) => {
                        // Prefer the symlink path for display, fall back to slave.
                        let desc = state
                            .ptys
                            .list()
                            .into_iter()
                            .find(|v| v.id == *vs_id)
                            .map(|v| {
                                v.symlink_path
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| v.slave_path.display().to_string())
                            })
                            .unwrap_or(description);
                        let st = state.clone();
                        let task_desc = desc.clone();
                        let handle = tokio::spawn(async move {
                            if let Err(e) = modbus_rtu::run_on_stream(st, stream).await {
                                tracing::error!("modbus rtu (virtual serial): {e}");
                            }
                        });
                        *self.rtu.lock() = Some(RtuRunning {
                            handle: handle.abort_handle(),
                            vs_id: Some(vs_id.clone()),
                            cfg,
                        });
                        self.status.lock().rtu = TransportState::Running {
                            description: task_desc,
                        };
                    }
                    Err(e) => {
                        self.status.lock().rtu = TransportState::Error {
                            description,
                            message: e.to_string(),
                        };
                    }
                }
                return;
            }
        }

        // ---- Physical serial path ---------------------------------------
        if cfg.device.is_empty() {
            self.status.lock().rtu = TransportState::Error {
                description: "(no device)".into(),
                message: "Device path is empty — pick a virtual serial or enter a device path."
                    .into(),
            };
            return;
        }
        let description = cfg.device.clone();
        let serial_cfg = modbus_rtu::SerialConfig {
            device: &cfg.device,
            baud_rate: if cfg.baud_rate == 0 {
                9600
            } else {
                cfg.baud_rate
            },
            data_bits: if cfg.data_bits == 0 { 8 } else { cfg.data_bits },
            stop_bits: if cfg.stop_bits == 0 { 1 } else { cfg.stop_bits },
            parity: if cfg.parity.is_empty() {
                "N"
            } else {
                &cfg.parity
            },
        };
        match modbus_rtu::open_serial(serial_cfg) {
            Ok(stream) => {
                let st = state.clone();
                let handle = tokio::spawn(async move {
                    if let Err(e) = modbus_rtu::run_on_stream(st, stream).await {
                        tracing::error!("modbus rtu serve: {e}");
                    }
                });
                *self.rtu.lock() = Some(RtuRunning {
                    handle: handle.abort_handle(),
                    vs_id: None,
                    cfg,
                });
                self.status.lock().rtu = TransportState::Running { description };
            }
            Err(e) => {
                tracing::error!("modbus rtu open {description} failed: {e}");
                self.status.lock().rtu = TransportState::Error {
                    description,
                    message: e.to_string(),
                };
            }
        }
    }
}

/// Replace the registry entry for `id` with a fresh PTY pair reusing the same
/// id + symlink (re-pointing the symlink at the new slave and re-applying the
/// 0666 mode). Recovers a PTY whose master fd was dropped — e.g. after an RTU
/// task abort, or a genuinely dead PTY left with a dangling symlink.
#[cfg(unix)]
fn rebuild_pty(state: &Arc<AppState>, id: &str) -> anyhow::Result<()> {
    let Some(info) = state.ptys.get(id) else {
        anyhow::bail!("virtual serial '{id}' not found");
    };
    let symlink = info.symlink_path.clone();
    state.ptys.remove(id);
    state.ptys.create_with_id(id.to_string(), symlink)?;
    // Persist the new state (in case symlink paths shifted).
    state.save_virtual_serials()?;
    Ok(())
}

/// Fire-and-forget wrapper around [`rebuild_pty`] for the reconfigure/stop
/// paths, where the user's saved RTU config (and their test app which opened
/// the symlink) must stay valid across toggles and restarts.
#[cfg(unix)]
fn recycle_pty(state: &Arc<AppState>, id: &str) {
    if let Err(e) = rebuild_pty(state, id) {
        tracing::warn!("failed to recycle virtual serial {id}: {e}");
    }
}

#[cfg(not(unix))]
fn recycle_pty(_state: &Arc<AppState>, _id: &str) {}
