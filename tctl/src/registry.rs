//! Per-user inventory of managed VMs (`~/.local/share/tctl/tctl.db`).
//!
//! Derived from the project config files — never the source of truth for
//! configuration, never committed, and rebuildable with `registry scan`.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::config::{self, Config};
use crate::Result;

#[derive(Debug, Clone, Serialize)]
pub struct VmRecord {
    pub name: String,
    pub template: Option<String>,
    pub project_dir: PathBuf,
    pub share_dir: Option<PathBuf>,
    pub remote_forward: Vec<String>,
    pub local_forward: Vec<String>,
    pub env_files: Vec<String>,
    pub last_ip: Option<String>,
    pub pid: Option<u32>,
    pub log_path: Option<PathBuf>,
    pub last_provision_hash: Option<String>,
    pub last_provision_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub registered: Vec<String>,
    pub failed: Vec<(PathBuf, String)>,
}

pub struct Registry {
    conn: Connection,
    pub path: PathBuf,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS vms (
    name                TEXT PRIMARY KEY,
    template            TEXT,
    project_dir         TEXT NOT NULL,
    share_dir           TEXT,
    remote_forward      TEXT NOT NULL DEFAULT '[]',
    local_forward       TEXT NOT NULL DEFAULT '[]',
    env_files           TEXT NOT NULL DEFAULT '[]',
    last_ip             TEXT,
    pid                 INTEGER,
    log_path            TEXT,
    last_provision_hash TEXT,
    last_provision_at   TEXT,
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);";

impl Registry {
    pub fn default_path() -> PathBuf {
        config::data_dir().join("tctl.db")
    }

    pub fn open_default() -> Result<Registry> {
        Self::open(&Self::default_path())
    }

    pub fn open(path: &Path) -> Result<Registry> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(SCHEMA)?;
        // Upgrade registries produced by the pre-release scaffold.
        let has_template: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('vms') WHERE name = 'template')",
            [],
            |row| row.get(0),
        )?;
        if !has_template {
            conn.execute_batch("ALTER TABLE vms ADD COLUMN template TEXT;")?;
        }
        Ok(Registry {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Inserts or refreshes the configuration-derived columns of a VM.
    pub fn upsert_config(&self, cfg: &Config) -> Result<()> {
        self.conn.execute(
            "INSERT INTO vms (name, project_dir, share_dir, remote_forward, local_forward, env_files, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             ON CONFLICT(name) DO UPDATE SET
               project_dir = excluded.project_dir, share_dir = excluded.share_dir,
               remote_forward = excluded.remote_forward, local_forward = excluded.local_forward,
               env_files = excluded.env_files, updated_at = excluded.updated_at",
            params![
                cfg.vm_name,
                cfg.project_dir.to_string_lossy(),
                cfg.share_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                serde_json::to_string(&cfg.remote_forward)?,
                serde_json::to_string(&cfg.local_forward)?,
                serde_json::to_string(&cfg.env_files)?,
            ],
        )?;
        Ok(())
    }

    pub fn record_start(&self, name: &str, pid: u32, log: &Path, ip: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE vms SET pid = ?2, log_path = ?3, last_ip = COALESCE(?4, last_ip),
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE name = ?1",
            params![name, pid, log.to_string_lossy(), ip],
        )?;
        Ok(())
    }

    pub fn record_ip(&self, name: &str, ip: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE vms SET last_ip = ?2 WHERE name = ?1",
            params![name, ip],
        )?;
        Ok(())
    }

    pub fn record_stop(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE vms SET pid = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE name = ?1",
            params![name],
        )?;
        Ok(())
    }

    pub fn record_provision(&self, name: &str, hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE vms SET last_provision_hash = ?2, last_provision_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE name = ?1",
            params![name, hash],
        )?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<VmRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("{SELECT} WHERE name = ?1"),
                params![name],
                row_to_record,
            )
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<VmRecord>> {
        let mut stmt = self.conn.prepare(&format!("{SELECT} ORDER BY name"))?;
        let rows = stmt.query_map([], row_to_record)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn remove(&self, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM vms WHERE name = ?1", params![name])?
            > 0)
    }

    /// Walks `root` for project markers and registers every valid one.
    /// Returns the registered VM names and the files that failed to load.
    pub fn scan(&self, root: &Path) -> Result<ScanReport> {
        if !root.is_dir() {
            return Err(crate::Error::Config(format!(
                "'{}' is not a directory",
                root.display()
            )));
        }
        let root = root.canonicalize()?;
        let mut found = Vec::new();
        let mut failed = Vec::new();
        for dir in walk_for_markers(&root) {
            match Config::load(&dir) {
                Ok(cfg) => {
                    self.upsert_config(&cfg)?;
                    found.push(cfg.vm_name);
                }
                Err(e) => failed.push((dir.join(config::PROJECT_FILE), e.to_string())),
            }
        }
        Ok(ScanReport {
            registered: found,
            failed,
        })
    }
}

const SELECT: &str = "SELECT name, project_dir, share_dir, remote_forward, local_forward, env_files,
                             last_ip, pid, log_path, last_provision_hash, last_provision_at, updated_at, template FROM vms";

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<VmRecord> {
    let json_list = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();
    Ok(VmRecord {
        name: row.get(0)?,
        template: row.get(12)?,
        project_dir: PathBuf::from(row.get::<_, String>(1)?),
        share_dir: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
        remote_forward: json_list(row.get(3)?),
        local_forward: json_list(row.get(4)?),
        env_files: json_list(row.get(5)?),
        last_ip: row.get(6)?,
        pid: row.get::<_, Option<i64>>(7)?.map(|p| p as u32),
        log_path: row.get::<_, Option<String>>(8)?.map(PathBuf::from),
        last_provision_hash: row.get(9)?,
        last_provision_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// Directories under `root` (inclusive) containing a project marker. Skips
/// hidden directories, `node_modules` and `target`; does not follow symlinks.
fn walk_for_markers(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir.join(config::PROJECT_FILE).is_file() {
            out.push(dir.clone());
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FileConfig;

    #[test]
    fn opens_pre_release_schema_without_losing_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("old.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&SCHEMA.replace("    template            TEXT,\n", ""))
            .unwrap();
        conn.execute(
            "INSERT INTO vms(name, project_dir) VALUES ('demo', '/project')",
            [],
        )
        .unwrap();
        drop(conn);
        let reg = Registry::open(&path).unwrap();
        assert_eq!(reg.get("demo").unwrap().unwrap().template, None);
    }

    #[test]
    fn roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::open(&tmp.path().join("t.db")).unwrap();
        let cfg = Config::from_layers(
            FileConfig::default(),
            FileConfig::parse("[vm]\nname='demo'\n[ssh]\nlocal_forward=['1 2']\n").unwrap(),
            tmp.path().join("g.toml"),
            false,
            tmp.path().to_path_buf(),
            tmp.path().join(config::PROJECT_FILE),
        )
        .unwrap();
        reg.upsert_config(&cfg).unwrap();
        reg.record_start("demo", 42, Path::new("/log"), Some("1.2.3.4"))
            .unwrap();
        reg.record_provision("demo", "abc").unwrap();
        let r = reg.get("demo").unwrap().unwrap();
        assert_eq!(r.template, None);
        assert_eq!(r.pid, Some(42));
        assert_eq!(r.last_ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(r.local_forward, vec!["1 2"]);
        assert_eq!(r.last_provision_hash.as_deref(), Some("abc"));
        assert_eq!(r.last_provision_at.as_deref(), Some(r.updated_at.as_str()));
        assert_eq!(reg.list().unwrap().len(), 1);
        assert!(reg.remove("demo").unwrap());
        assert!(reg.get("demo").unwrap().is_none());
    }
}
