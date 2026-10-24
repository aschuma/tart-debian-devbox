use serde_json::Value;

mod common;
use common::Sandbox;

trait SandboxJson {
    fn json(&self, args: &[&str]) -> Value;
}

impl SandboxJson for Sandbox {
    fn json(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[test]
fn bare_command_displays_help_successfully_without_config() {
    let s = Sandbox::new();
    let out = s.run(&[]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
}

#[test]
fn init_is_scriptable_and_registers_without_overwriting() {
    let s = Sandbox::new();
    let init = s.json(&["--json", "init", "--vm", "demo", "--share", "auto"]);
    assert_eq!(init["result"]["vm"], "demo");
    let list = s.json(&["--json", "registry", "list"]);
    assert_eq!(list["result"][0]["name"], "demo");
    let original = std::fs::read(s.project.join(".tart-ctl.toml")).unwrap();
    assert!(!s
        .run(&["init", "--vm", "other", "--share", "none"])
        .status
        .success());
    assert_eq!(
        original,
        std::fs::read(s.project.join(".tart-ctl.toml")).unwrap()
    );
    assert!(s.dir.path().exists());
}

#[test]
fn env_json_masks_values_and_diff_by_default() {
    let s = Sandbox::new();
    s.project("[vm]\nname='demo'\n[ssh]\nuser='devbox'\nconfig_path='~/.ssh/config'\nidentity_file='~/key'\n");
    std::fs::write(
        s.project.join(".env"),
        "API_TOKEN=sentinel-private-value\nPLAIN=ok\n",
    )
    .unwrap();
    let masked = s.json(&["--json", "env"]);
    assert!(!masked.to_string().contains("sentinel-private-value"));
    assert_eq!(masked["result"]["effective"][0]["value"], "********");
    let revealed = s.json(&["--json", "env", "--show-secrets"]);
    assert_eq!(
        revealed["result"]["effective"][0]["value"],
        "sentinel-private-value"
    );
    let vm = s.world.join("vms/demo");
    std::fs::create_dir_all(&vm).unwrap();
    std::fs::write(vm.join("state"), "running").unwrap();
    std::fs::write(vm.join("ip"), "192.168.64.2").unwrap();
    let diff = s.run(&["--json", "env", "--diff"]);
    assert_eq!(diff.status.code(), Some(1));
    let diff: Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(diff["result"]["diff"]["up_to_date"], false);
    assert!(!diff.to_string().contains("sentinel-private-value"));
    s.json(&["--json", "provision"]);
    let current = s.json(&["--json", "env", "--diff"]);
    assert_eq!(current["result"]["diff"]["up_to_date"], true);
    let registry = s.json(&["--json", "registry", "show", "demo"]);
    assert!(registry["result"]["last_provision_hash"].as_str().is_some());
    assert!(!registry.to_string().contains("sentinel-private-value"));
}

#[test]
fn legacy_global_config_is_rejected_instead_of_silently_ignored() {
    let s = Sandbox::new();
    s.project("[vm]\nname='demo'\n");
    std::fs::create_dir_all(s.home.join(".config/tctl")).unwrap();
    std::fs::write(s.home.join(".config/tctl/config"), "TCTL_SSH_USER=devbox\n").unwrap();
    let out = s.run(&["--json", "status"]);
    assert_eq!(out.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("Legacy config"));
}

#[test]
fn config_rejects_names_that_could_escape_log_directory_or_add_ssh_options() {
    let s = Sandbox::new();
    for name in ["../escape", "-oProxyCommand=bad", "demo\nHost *", ".", ".."] {
        s.project(&format!(
            "[vm]\nname={}\n",
            toml::Value::String(name.into())
        ));
        let out = s.run(&["--json", "status"]);
        assert_eq!(out.status.code(), Some(1), "accepted {name:?}");
        let error: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(error["ok"], false);
    }
}

#[test]
fn json_argument_errors_are_machine_readable() {
    let s = Sandbox::new();
    let out = s.run(&["--json", "env", "--invalid"]);
    assert_eq!(out.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(error["ok"], false);
}
