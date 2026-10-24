//! Shared sandbox for integration tests: isolated HOME/XDG dirs, a project
//! directory, and the fake `tart`/`ssh` world (tests/fakes) on PATH.

use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::TempDir;

pub struct Sandbox {
    /// Owns the temp tree; dropping it removes everything.
    #[allow(dead_code)]
    pub dir: TempDir,
    pub home: PathBuf,
    pub project: PathBuf,
    pub world: PathBuf,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let project = dir.path().join("project");
        let world = dir.path().join("world");
        for path in [&home, &project, &world] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(world.join("calls.log"), "").unwrap();
        Self {
            dir,
            home,
            project,
            world,
        }
    }

    /// A `tctl` command configured for the sandbox (fakes first on PATH).
    pub fn cmd(&self, args: &[&str]) -> Command {
        let fakes = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fakes");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tctl"));
        cmd.args(args)
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env(
                "PATH",
                format!("{}:{}", fakes.display(), std::env::var("PATH").unwrap()),
            )
            .env("TCTL_FAKE_WORLD", &self.world)
            .env("NO_COLOR", "1")
            .env_remove("RUST_LOG")
            .stdin(std::process::Stdio::null());
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    pub fn project(&self, text: &str) {
        std::fs::write(self.project.join(".tart-ctl.toml"), text).unwrap();
    }

    /// Defines a VM in the fake tart world. `ip: None` removes any previous
    /// IP so `tart ip` fails — callers rely on that to simulate a VM without
    /// an address.
    #[allow(dead_code)] // each integration test binary compiles its own copy
    pub fn vm(&self, name: &str, state: &str, ip: Option<&str>) {
        let dir = self.world.join("vms").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("state"), state).unwrap();
        match ip {
            Some(ip) => std::fs::write(dir.join("ip"), ip).unwrap(),
            None => {
                let _ = std::fs::remove_file(dir.join("ip"));
            }
        }
    }
}
