//! `tctl` core library: configuration, Tart/ssh backend, env provisioning,
//! `~/.ssh/config` management and the per-user VM registry.
//!
//! Everything here is synchronous and free of terminal interaction; the CLI
//! (`main.rs`) and the HTTP/WebSocket server (`server`) are thin front-ends
//! over these types.

pub mod backend;
pub mod config;
pub mod envfile;
pub mod error;
pub mod ops;
pub mod registry;
pub mod server;
pub mod sshconfig;

pub use error::{Error, Result};

/// Destination of the provisioned environment inside the VM.
pub const ENV_DEST: &str = "/etc/profile.d/tart-provision.sh";
/// Mount point of the host share inside the VM.
pub const SHARE_MOUNT: &str = "/mnt/shared/workspace";
