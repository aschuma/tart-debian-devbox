//! Layered TOML configuration: `~/.config/tctl/config.toml` (global defaults)
//! overridden per key by `<project>/.tart-ctl.toml` (found by walking upward).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const PROJECT_FILE: &str = ".tart-ctl.toml";
pub const GLOBAL_FILE: &str = "config.toml";
pub const DEFAULT_ENV_FILES: &[&str] = &[".env"];

// ─── On-disk shape ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provision: Option<ProvisionSection>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VmSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShareSection {
    /// `"auto"` (project root), a path, or `""` / `"none"` for no share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SshSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_forward: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_forward: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProvisionSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_files: Option<Vec<String>>,
}

impl FileConfig {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }

    /// Returns `self` with every key that `over` sets replaced by `over`'s value.
    pub fn layered(self, over: FileConfig) -> FileConfig {
        /// Combines two optional sections; a section present on both sides is
        /// merged per key, otherwise whichever side has one wins.
        fn section<T>(
            base: Option<T>,
            over: Option<T>,
            merge: impl FnOnce(T, T) -> T,
        ) -> Option<T> {
            match (base, over) {
                (Some(b), Some(o)) => Some(merge(b, o)),
                (b, o) => o.or(b),
            }
        }
        fn pick<T>(base: Option<T>, over: Option<T>) -> Option<T> {
            over.or(base)
        }
        FileConfig {
            vm: section(self.vm, over.vm, |b, o| VmSection {
                name: pick(b.name, o.name),
            }),
            share: section(self.share, over.share, |b, o| ShareSection {
                dir: pick(b.dir, o.dir),
            }),
            ssh: section(self.ssh, over.ssh, |b, o| SshSection {
                user: pick(b.user, o.user),
                config_path: pick(b.config_path, o.config_path),
                identity_file: pick(b.identity_file, o.identity_file),
                remote_forward: pick(b.remote_forward, o.remote_forward),
                local_forward: pick(b.local_forward, o.local_forward),
            }),
            provision: section(self.provision, over.provision, |b, o| ProvisionSection {
                env_files: pick(b.env_files, o.env_files),
            }),
        }
    }
}

// ─── Locations ───────────────────────────────────────────────────────────────

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("tctl")
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("tctl")
}

pub fn global_config_path() -> PathBuf {
    config_dir().join(GLOBAL_FILE)
}

pub fn load_global_config() -> Result<(FileConfig, bool)> {
    let path = global_config_path();
    if path.is_file() {
        return Ok((FileConfig::read(&path)?, true));
    }
    Ok((FileConfig::default(), false))
}

pub fn valid_vm_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Expands a leading `~` / `~/` or `$HOME` in a path-like value.
pub fn expand_home(value: &str) -> PathBuf {
    let home = home_dir();
    if value == "~" {
        home
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else if let Some(rest) = value.strip_prefix("${HOME}") {
        PathBuf::from(format!("{}{}", home.display(), rest))
    } else if value == "$HOME" {
        home
    } else if let Some(rest) = value.strip_prefix("$HOME/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    }
}

/// Walks upward from `start` looking for the project marker.
/// Returns the directory containing it.
pub fn find_project_dir(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(PROJECT_FILE).is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    Err(Error::NoProjectMarker {
        marker: PROJECT_FILE.into(),
        from: start.to_path_buf(),
    })
}

// ─── Effective configuration ─────────────────────────────────────────────────

/// Fully resolved configuration for one project.
#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub vm_name: String,
    /// Resolved host share directory; `None` = start the VM without a share.
    pub share_dir: Option<PathBuf>,
    pub ssh_user: Option<String>,
    pub ssh_config: Option<PathBuf>,
    pub identity_file: Option<PathBuf>,
    pub remote_forward: Vec<String>,
    pub local_forward: Vec<String>,
    /// Raw env file entries (relative to `project_dir`, `~/…`, or absolute).
    pub env_files: Vec<String>,
    pub global_path: PathBuf,
    pub global_exists: bool,
    pub project_path: PathBuf,
    pub project_dir: PathBuf,
}

/// Essential ssh settings, present once `Config::ssh()` succeeded.
#[derive(Debug, Clone)]
pub struct SshSettings {
    pub user: String,
    pub config: PathBuf,
    pub identity_file: PathBuf,
}

impl Config {
    /// Loads global + project config starting the upward search at `cwd`.
    pub fn load(cwd: &Path) -> Result<Config> {
        let global_path = global_config_path();
        let (global, global_exists) = load_global_config()?;
        let project_dir = find_project_dir(cwd)?;
        let project_path = project_dir.join(PROJECT_FILE);
        let project = FileConfig::read(&project_path)?;
        Self::from_layers(
            global,
            project,
            global_path,
            global_exists,
            project_dir,
            project_path,
        )
    }

    pub fn from_layers(
        global: FileConfig,
        project: FileConfig,
        global_path: PathBuf,
        global_exists: bool,
        project_dir: PathBuf,
        project_path: PathBuf,
    ) -> Result<Config> {
        let vm_name = project
            .vm
            .clone()
            .and_then(|v| v.name)
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Error::MissingVmName {
                file: project_path.clone(),
                dir: project_dir.clone(),
            })?;

        if !valid_vm_name(&vm_name) {
            return Err(Error::Config(format!("Invalid VM name '{vm_name}': use letters, digits, '.', '_' and '-', with no leading '-' or path components.")));
        }
        let merged = global.layered(project);
        let resolve_path = |value: &str| {
            let path = expand_home(value);
            if path.is_absolute() {
                path
            } else {
                project_dir.join(path)
            }
        };
        let share_dir = match merged.share.and_then(|s| s.dir).as_deref() {
            None | Some("") | Some("none") => None,
            Some("auto") => Some(project_dir.clone()),
            Some(p) => Some(resolve_path(p)),
        };
        if let Some(dir) = &share_dir {
            if !dir.is_dir() {
                return Err(Error::ShareDirNotADirectory(dir.clone()));
            }
        }

        let ssh = merged.ssh.unwrap_or_default();
        let non_empty = |s: Option<String>| s.filter(|v| !v.is_empty());
        let env_files = merged
            .provision
            .and_then(|p| p.env_files)
            .unwrap_or_else(|| DEFAULT_ENV_FILES.iter().map(|s| s.to_string()).collect());

        Ok(Config {
            vm_name,
            share_dir,
            ssh_user: non_empty(ssh.user),
            ssh_config: non_empty(ssh.config_path).map(|p| resolve_path(&p)),
            identity_file: non_empty(ssh.identity_file).map(|p| resolve_path(&p)),
            remote_forward: clean_list(ssh.remote_forward),
            local_forward: clean_list(ssh.local_forward),
            env_files,
            global_path,
            global_exists,
            project_path,
            project_dir,
        })
    }

    /// The ssh settings, or an error naming what is missing.
    pub fn ssh(&self) -> Result<SshSettings> {
        match (&self.ssh_user, &self.ssh_config, &self.identity_file) {
            (Some(user), Some(config), Some(identity_file)) => Ok(SshSettings {
                user: user.clone(),
                config: config.clone(),
                identity_file: identity_file.clone(),
            }),
            _ => {
                let missing = [
                    ("ssh.user", self.ssh_user.is_none()),
                    ("ssh.config_path", self.ssh_config.is_none()),
                    ("ssh.identity_file", self.identity_file.is_none()),
                ]
                .into_iter()
                .filter_map(|(name, absent)| absent.then_some(name))
                .collect::<Vec<_>>()
                .join(" ");
                Err(Error::MissingSshSettings(missing))
            }
        }
    }

    /// Resolves one `env_files` entry to an absolute path (`None` for blank entries).
    pub fn resolve_env_path(&self, entry: &str) -> Option<PathBuf> {
        let entry = entry.trim();
        if entry.is_empty() {
            return None;
        }
        let path = expand_home(entry);
        Some(if path.is_absolute() {
            path
        } else {
            self.project_dir.join(path)
        })
    }

    /// Where `start` writes the detached `tart run` output.
    pub fn log_path(&self) -> PathBuf {
        data_dir()
            .join("logs")
            .join(format!("{}.log", self.vm_name))
    }
}

fn clean_list(list: Option<Vec<String>>) -> Vec<String> {
    list.unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(global: &str, project: &str, dir: &Path) -> Result<Config> {
        Config::from_layers(
            FileConfig::parse(global).unwrap(),
            FileConfig::parse(project).unwrap(),
            PathBuf::from("/g/config.toml"),
            true,
            dir.to_path_buf(),
            dir.join(PROJECT_FILE),
        )
    }

    #[test]
    fn project_overrides_global_per_key() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg(
            "[ssh]\nuser='devbox'\nconfig_path='~/.ssh/config'\nidentity_file='~/k'\nremote_forward=['1080 127.0.0.1:1080']\n[share]\ndir='auto'\n",
            "[vm]\nname='demo'\n[ssh]\nlocal_forward=['5432 127.0.0.1:5432']\n",
            tmp.path(),
        )
        .unwrap();
        assert_eq!(c.vm_name, "demo");
        assert_eq!(c.share_dir.as_deref(), Some(tmp.path()));
        assert_eq!(c.remote_forward, vec!["1080 127.0.0.1:1080"]);
        assert_eq!(c.local_forward, vec!["5432 127.0.0.1:5432"]);
        assert_eq!(c.env_files, vec![".env"]);
        assert_eq!(c.ssh().unwrap().user, "devbox");
    }

    #[test]
    fn missing_vm_name_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            cfg("", "[share]\ndir='auto'\n", tmp.path()),
            Err(Error::MissingVmName { .. })
        ));
    }

    #[test]
    fn share_none_and_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(cfg("", "[vm]\nname='d'\n[share]\ndir=''\n", tmp.path())
            .unwrap()
            .share_dir
            .is_none());
        assert!(cfg("", "[vm]\nname='d'\n[share]\ndir='none'\n", tmp.path())
            .unwrap()
            .share_dir
            .is_none());
        assert!(matches!(
            cfg(
                "",
                "[vm]\nname='d'\n[share]\ndir='/definitely/not/here'\n",
                tmp.path()
            ),
            Err(Error::ShareDirNotADirectory(_))
        ));
    }

    #[test]
    fn env_path_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg("", "[vm]\nname='d'\n", tmp.path()).unwrap();
        assert_eq!(c.resolve_env_path(" .env "), Some(tmp.path().join(".env")));
        assert_eq!(c.resolve_env_path("/abs"), Some(PathBuf::from("/abs")));
        assert_eq!(c.resolve_env_path("~/x"), Some(home_dir().join("x")));
        assert_eq!(c.resolve_env_path("$HOME/x"), Some(home_dir().join("x")));
        assert_eq!(c.resolve_env_path("${HOME}/x"), Some(home_dir().join("x")));
        assert_eq!(
            c.resolve_env_path("$HOME_OTHER/x"),
            Some(tmp.path().join("$HOME_OTHER/x"))
        );
        assert_eq!(c.resolve_env_path("  "), None);
    }

    #[test]
    fn missing_ssh_settings_named() {
        let tmp = tempfile::tempdir().unwrap();
        let c = cfg("", "[vm]\nname='d'\n[ssh]\nuser='u'\n", tmp.path()).unwrap();
        match c.ssh() {
            Err(Error::MissingSshSettings(s)) => assert_eq!(s, "ssh.config_path ssh.identity_file"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn finds_marker_upward() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(tmp.path().join(PROJECT_FILE), "").unwrap();
        assert_eq!(find_project_dir(&deep).unwrap(), tmp.path());

        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            find_project_dir(empty.path()),
            Err(Error::NoProjectMarker { .. })
        ));
    }
}
