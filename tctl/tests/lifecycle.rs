use std::cell::{Cell, RefCell};
use std::path::Path;

use tctl::backend::{RemoteOutput, TartBackend, VmInfo, VmState};
use tctl::config::{Config, FileConfig, SshSettings, PROJECT_FILE};
use tctl::ops::{self, Collected, Ctx};
use tctl::registry::Registry;
use tctl::{Error, Result};

struct FakeBackend {
    state: Cell<Option<VmState>>,
    ip: RefCell<Option<String>>,
    starts: Cell<usize>,
    stops: Cell<usize>,
}

impl FakeBackend {
    fn new(state: Option<VmState>, ip: Option<&str>) -> Self {
        Self {
            state: Cell::new(state),
            ip: RefCell::new(ip.map(str::to_owned)),
            starts: Cell::new(0),
            stops: Cell::new(0),
        }
    }
}

impl TartBackend for FakeBackend {
    fn list(&self) -> Result<Vec<VmInfo>> {
        Ok(self
            .state
            .get()
            .into_iter()
            .map(|state| VmInfo {
                name: "demo".into(),
                state,
                state_raw: if state == VmState::Running {
                    "running"
                } else {
                    "stopped"
                }
                .into(),
                source: "local".into(),
            })
            .collect())
    }
    fn ip(&self, _: &str) -> Result<Option<String>> {
        Ok(self.ip.borrow().clone())
    }
    fn run_detached(&self, _: &str, _: Option<&Path>, _: &Path) -> Result<u32> {
        self.starts.set(self.starts.get() + 1);
        self.state.set(Some(VmState::Running));
        Ok(42)
    }
    fn stop(&self, _: &str) -> Result<()> {
        self.stops.set(self.stops.get() + 1);
        self.state.set(Some(VmState::Stopped));
        Ok(())
    }
    fn run_remote(
        &self,
        _: &SshSettings,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<RemoteOutput> {
        unreachable!()
    }
    fn exec_ssh(&self, _: &SshSettings, _: &str, _: &[String]) -> Result<std::convert::Infallible> {
        unreachable!()
    }
}

fn config(dir: &Path) -> Config {
    Config::from_layers(
        FileConfig::default(),
        FileConfig::parse("[vm]\nname='demo'\n").unwrap(),
        dir.join("global.toml"),
        false,
        dir.into(),
        dir.join(PROJECT_FILE),
    )
    .unwrap()
}

#[test]
fn running_vm_without_ip_is_not_started_again() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let backend = FakeBackend::new(Some(VmState::Running), None);
    let mut messages = Collected::default();
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &backend,
        registry: None,
        reporter: &mut messages,
    };
    assert!(ops::start(&mut ctx, 0).unwrap().already_running);
    assert_eq!(backend.starts.get(), 0);
}

#[test]
fn running_vm_without_ip_can_be_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let backend = FakeBackend::new(Some(VmState::Running), None);
    let mut messages = Collected::default();
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &backend,
        registry: None,
        reporter: &mut messages,
    };
    assert!(ops::stop(&mut ctx).unwrap().was_running);
    assert_eq!(backend.stops.get(), 1);
}

#[test]
fn startup_timeout_preserves_pid_and_log_for_diagnosis() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let backend = FakeBackend::new(Some(VmState::Stopped), None);
    let registry = Registry::open(&dir.path().join("registry.db")).unwrap();
    let mut messages = Collected::default();
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &backend,
        registry: Some(&registry),
        reporter: &mut messages,
    };
    assert!(matches!(
        ops::start(&mut ctx, 1),
        Err(Error::StartTimeout(_, 1))
    ));
    let row = registry
        .get("demo")
        .unwrap()
        .expect("registered before waiting");
    assert_eq!(row.pid, Some(42));
    assert_eq!(row.log_path, Some(cfg.log_path()));
}

#[test]
fn start_refreshes_registry_even_if_vm_was_already_running() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let backend = FakeBackend::new(Some(VmState::Running), Some("192.168.64.2"));
    let registry = Registry::open(&dir.path().join("registry.db")).unwrap();
    let mut messages = Collected::default();
    let mut ctx = Ctx {
        cfg: &cfg,
        backend: &backend,
        registry: Some(&registry),
        reporter: &mut messages,
    };
    assert!(ops::start(&mut ctx, 1).unwrap().already_running);
    assert_eq!(
        registry.get("demo").unwrap().unwrap().last_ip.as_deref(),
        Some("192.168.64.2")
    );
}
