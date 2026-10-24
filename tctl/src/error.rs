use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No '{marker}' found (searched from '{from}' upward to '/').\n       Create one in the root of the project you want to manage:\n         cd <project-root> && tctl init")]
    NoProjectMarker { marker: String, from: PathBuf },

    #[error("Legacy config '{0}' found. Convert it to TOML with scripts/migrate-tctl-env.sh")]
    LegacyConfig(PathBuf),

    #[error("'vm.name' is not set in '{file}'.\n       Each project names its own VM there:\n         [vm]\n         name = \"my-project\"\n       Or regenerate the file:   cd '{dir}' && tctl init")]
    MissingVmName { file: PathBuf, dir: PathBuf },

    #[error("Missing settings: {0}.\n       Set them in the global config (or override in the project config).")]
    MissingSshSettings(String),

    #[error("share.dir '{0}' is not a directory.")]
    ShareDirNotADirectory(PathBuf),

    #[error("VM '{0}' is not running. Start it with: tctl up")]
    VmNotRunning(String),

    #[error("VM '{0}' did not report an IP within {1} seconds")]
    StartTimeout(String, u64),

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    Backend(String),

    #[error("Failed to render env layers: {0}")]
    EnvRender(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Toml(#[from] toml::de::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
