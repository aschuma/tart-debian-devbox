//! Boundary to the outside world: the `tart` and `ssh` binaries.
//!
//! Everything that spawns a process lives behind [`TartBackend`] so the
//! operations in [`crate::ops`] can be tested with a fake and reused by the
//! HTTP server unchanged.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::SshSettings;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Running,
    Stopped,
    /// Tart reports something we don't model (e.g. "suspended").
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmInfo {
    pub name: String,
    pub state: VmState,
    /// Tart's literal state string.
    pub state_raw: String,
    pub source: String,
}

/// Captured result of a non-interactive remote command.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait TartBackend {
    fn list(&self) -> Result<Vec<VmInfo>>;
    /// IP of a running VM, `None` if Tart cannot report one.
    fn ip(&self, name: &str) -> Result<Option<String>>;
    /// Spawns `tart run` detached from this process; returns its PID.
    fn run_detached(&self, name: &str, share: Option<&Path>, log: &Path) -> Result<u32>;
    fn stop(&self, name: &str) -> Result<()>;
    /// Runs a command on the VM through the ssh config `Host` alias.
    fn run_remote(
        &self,
        ssh: &SshSettings,
        host: &str,
        cmd: &str,
        stdin: Option<&str>,
    ) -> Result<RemoteOutput>;
    /// Replaces the current process with an ssh session (never returns on success).
    fn exec_ssh(
        &self,
        ssh: &SshSettings,
        host: &str,
        remote_cmd: &[String],
    ) -> Result<std::convert::Infallible>;

    fn state_of(&self, name: &str) -> Result<Option<VmInfo>> {
        Ok(self
            .list()?
            .into_iter()
            .find(|v| v.name == name && v.source == "local"))
    }
}

/// Real implementation shelling out to `tart` and `ssh`.
#[derive(Debug, Default, Clone)]
pub struct ProcessBackend;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TartListEntry {
    name: String,
    state: String,
    source: String,
}

impl TartBackend for ProcessBackend {
    fn list(&self) -> Result<Vec<VmInfo>> {
        let out = Command::new("tart")
            .args(["list", "--format", "json"])
            .output()
            .map_err(tart_err)?;
        if !out.status.success() {
            return Err(Error::Backend(format!(
                "tart list failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let entries: Vec<TartListEntry> = serde_json::from_slice(&out.stdout)?;
        Ok(entries
            .into_iter()
            .map(|e| VmInfo {
                state: match e.state.as_str() {
                    "running" => VmState::Running,
                    "stopped" => VmState::Stopped,
                    _ => VmState::Other,
                },
                state_raw: e.state,
                name: e.name,
                source: e.source,
            })
            .collect())
    }

    fn ip(&self, name: &str) -> Result<Option<String>> {
        let out = Command::new("tart")
            .args(["ip", name])
            .output()
            .map_err(tart_err)?;
        if !out.status.success() {
            return Ok(None);
        }
        let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if ip.is_empty() { None } else { Some(ip) })
    }

    fn run_detached(&self, name: &str, share: Option<&Path>, log: &Path) -> Result<u32> {
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        let log_err = log_file.try_clone()?;

        let mut cmd = Command::new("tart");
        cmd.arg("run").arg("--no-graphics");
        if let Some(dir) = share {
            cmd.arg(format!("--dir=workspace:{}", dir.display()));
        }
        cmd.arg(name)
            .stdin(Stdio::null())
            .stdout(log_file)
            .stderr(log_err);

        // Detach: new session so the VM outlives this process and ignores its terminal.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid() is async-signal-safe and has no preconditions.
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let child = cmd.spawn().map_err(tart_err)?;
        Ok(child.id())
    }

    fn stop(&self, name: &str) -> Result<()> {
        let out = Command::new("tart")
            .args(["stop", name])
            .output()
            .map_err(tart_err)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Backend(format!(
                "tart stop {name} failed with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn run_remote(
        &self,
        ssh: &SshSettings,
        host: &str,
        cmd: &str,
        stdin: Option<&str>,
    ) -> Result<RemoteOutput> {
        let mut child = Command::new("ssh")
            .arg("-T")
            .arg("-F")
            .arg(&ssh.config)
            .arg("-i")
            .arg(&ssh.identity_file)
            .arg(format!("{}@{}", ssh.user, host))
            .arg(cmd)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Backend(format!("cannot run ssh: {e}")))?;
        // Feed stdin from its own thread: writing a large payload while the
        // child fills its stdout/stderr pipes would otherwise deadlock. A
        // write error (remote died early) is reflected in the exit status.
        let writer = stdin.map(|input| {
            let mut pipe = child.stdin.take().expect("piped stdin");
            let input = input.to_owned();
            std::thread::spawn(move || {
                let _ = pipe.write_all(input.as_bytes());
            })
        });
        let out = child.wait_with_output()?;
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        Ok(RemoteOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn exec_ssh(
        &self,
        ssh: &SshSettings,
        host: &str,
        remote_cmd: &[String],
    ) -> Result<std::convert::Infallible> {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("ssh");
        cmd.arg("-F")
            .arg(&ssh.config)
            .arg("-i")
            .arg(&ssh.identity_file);
        if remote_cmd.is_empty() {
            cmd.arg("-t");
        } else {
            cmd.arg("-T");
        }
        cmd.arg(format!("{}@{}", ssh.user, host));
        cmd.args(remote_cmd);
        let err = cmd.exec();
        Err(Error::Backend(format!("cannot exec ssh: {err}")))
    }
}

fn tart_err(e: std::io::Error) -> Error {
    Error::Backend(format!("cannot run tart: {e}"))
}
