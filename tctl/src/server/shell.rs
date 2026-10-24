//! Interactive shell sessions: WebSocket ⇄ PTY-backed `ssh`.
//!
//! Protocol: binary frames carry raw terminal bytes in both directions; text
//! frames carry JSON control messages (`{"type":"resize","cols":…,"rows":…}`
//! from the client, `{"type":"exit",…}` from the server before close).
//! Terminal traffic is never logged. Disconnecting kills the session's ssh
//! process, never the VM.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;

use super::{lookup, project_config, ApiError, AppState};
use crate::ops::{self, Collected, Ctx, SshTarget};
use crate::registry::Registry;

const MAX_SHELLS_TOTAL: usize = 16;
const MAX_SHELLS_PER_VM: usize = 4;
/// Upper bound for a single client frame (keystrokes are tiny; pastes are capped).
const MAX_CLIENT_MESSAGE: usize = 64 * 1024;
const HEARTBEAT: Duration = Duration::from_secs(30);
/// A healthy client answers pings, so silence this long means a dead peer.
/// This is liveness detection, not an idle logout.
const DEAD_PEER: Duration = Duration::from_secs(90);

// ─── Session accounting ──────────────────────────────────────────────────────

#[derive(Default)]
pub(crate) struct ShellLimits {
    inner: Mutex<Counts>,
}

#[derive(Default)]
struct Counts {
    total: usize,
    per_vm: HashMap<String, usize>,
}

pub(crate) struct ShellSlot {
    limits: Arc<ShellLimits>,
    vm: String,
}

impl ShellLimits {
    fn try_acquire(self: &Arc<Self>, vm: &str) -> Option<ShellSlot> {
        let mut counts = self.inner.lock().unwrap();
        let per_vm = counts.per_vm.get(vm).copied().unwrap_or(0);
        if counts.total >= MAX_SHELLS_TOTAL || per_vm >= MAX_SHELLS_PER_VM {
            return None;
        }
        counts.total += 1;
        *counts.per_vm.entry(vm.to_string()).or_default() += 1;
        Some(ShellSlot {
            limits: self.clone(),
            vm: vm.to_string(),
        })
    }
}

impl Drop for ShellSlot {
    fn drop(&mut self) {
        let mut counts = self.limits.inner.lock().unwrap();
        counts.total -= 1;
        if let Some(n) = counts.per_vm.get_mut(&self.vm) {
            *n -= 1;
            if *n == 0 {
                counts.per_vm.remove(&self.vm);
            }
        }
    }
}

// ─── Handshake ───────────────────────────────────────────────────────────────

pub(crate) async fn shell(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let target = {
        let (state, name) = (state.clone(), name.clone());
        tokio::task::spawn_blocking(move || prepare(&state, &name)).await
    };
    let target = match target {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return e.into_response(),
        Err(e) => return ApiError::internal(format!("worker failed: {e}")).into_response(),
    };
    let Some(slot) = state.shells.try_acquire(&name) else {
        return ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!("shell limit reached ({MAX_SHELLS_PER_VM} per VM, {MAX_SHELLS_TOTAL} total)"),
        )
        .into_response();
    };
    ws.max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| session(socket, target, slot))
}

/// Registered + running + fresh ssh config, or the error the client sees.
/// Never starts the VM implicitly.
fn prepare(state: &AppState, name: &str) -> Result<SshTarget, ApiError> {
    let row = lookup(name)?;
    let cfg = project_config(&row, name)?;
    let registry = Registry::open_default().ok();
    let mut collected = Collected::default();
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &*state.backend,
        registry: registry.as_ref(),
        reporter: &mut collected,
    };
    ops::prepare_ssh(&mut ctx).map_err(ApiError::from)
}

// ─── Session ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Control {
    Resize { cols: u16, rows: u16 },
}

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn Write + Send>,
}

fn spawn_pty(target: &SshTarget) -> Result<PtySession, ApiError> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| ApiError::internal(format!("cannot open pty: {e}")))?;
    let mut cmd = CommandBuilder::new("ssh");
    cmd.arg("-F");
    cmd.arg(&target.settings.config);
    cmd.arg("-i");
    cmd.arg(&target.settings.identity_file);
    cmd.arg(format!("{}@{}", target.settings.user, target.host));
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| ApiError::internal(format!("cannot spawn ssh: {e}")))?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| ApiError::internal(format!("cannot read pty: {e}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| ApiError::internal(format!("cannot write pty: {e}")))?;
    Ok(PtySession {
        master: pair.master,
        child,
        reader,
        writer,
    })
}

async fn session(mut socket: WebSocket, target: SshTarget, _slot: ShellSlot) {
    let pty = match tokio::task::spawn_blocking(move || spawn_pty(&target)).await {
        Ok(Ok(p)) => p,
        _ => {
            let _ = socket
                .send(Message::Text(
                    r#"{"type":"exit","error":"failed to start ssh"}"#.into(),
                ))
                .await;
            return;
        }
    };
    let PtySession {
        master,
        mut child,
        mut reader,
        writer,
    } = pty;
    let master = Arc::new(Mutex::new(master));

    // PTY → WS: a bounded channel, so a slow client backpressures the reader
    // thread instead of buffering unbounded terminal output.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // WS → PTY: dedicated writer thread; dropping the sender ends it.
    let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    std::thread::spawn(move || {
        let mut writer = writer;
        while let Some(bytes) = in_rx.blocking_recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_peer_activity = Instant::now();

    loop {
        tokio::select! {
            chunk = out_rx.recv() => match chunk {
                Some(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                None => break, // ssh exited (or pty closed)
            },
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(message)) => {
                    last_peer_activity = Instant::now();
                    match message {
                        Message::Binary(bytes) => {
                            if in_tx.send(bytes.to_vec()).await.is_err() {
                                break;
                            }
                        }
                        Message::Text(text) => {
                            if let Ok(Control::Resize { cols, rows }) = serde_json::from_str(&text) {
                                let size = PtySize {
                                    rows: rows.clamp(1, 500),
                                    cols: cols.clamp(1, 500),
                                    pixel_width: 0,
                                    pixel_height: 0,
                                };
                                let _ = master.lock().unwrap().resize(size);
                            }
                        }
                        // Pings are answered by the WebSocket layer; pongs
                        // already refreshed last_peer_activity above.
                        _ => {}
                    }
                }
            },
            _ = heartbeat.tick() => {
                if last_peer_activity.elapsed() > DEAD_PEER {
                    break;
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    drop(in_tx);
    let exit = child.try_wait().unwrap_or(None);
    let notice = serde_json::json!({
        "type": "exit",
        "exited": exit.is_some(),
        "exit_code": exit.as_ref().map(|s| s.exit_code()),
    });
    let _ = socket.send(Message::Text(notice.to_string().into())).await;
    let _ = socket.send(Message::Close(None)).await;
    // Kill this session's ssh only; the VM keeps running.
    let _ = tokio::task::spawn_blocking(move || {
        let _ = child.kill();
        let _ = child.wait();
        drop(master);
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_enforce_per_vm_and_total_caps() {
        let limits = Arc::new(ShellLimits::default());
        let per_vm: Vec<_> = (0..MAX_SHELLS_PER_VM)
            .map(|_| limits.try_acquire("a").unwrap())
            .collect();
        assert!(limits.try_acquire("a").is_none(), "per-VM cap ignored");
        let mut rest = Vec::new();
        for i in 0..(MAX_SHELLS_TOTAL - MAX_SHELLS_PER_VM) {
            rest.push(
                limits
                    .try_acquire(&format!("vm{}", i / MAX_SHELLS_PER_VM))
                    .unwrap(),
            );
        }
        assert!(limits.try_acquire("fresh").is_none(), "total cap ignored");
        drop(per_vm);
        assert!(limits.try_acquire("a").is_some(), "slots not released");
        drop(rest);
    }
}
