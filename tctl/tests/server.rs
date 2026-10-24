//! End-to-end tests of `tctl server`: real binary, real sockets, fake tart/ssh.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::Message;

mod common;
use common::Sandbox;

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server(sandbox: &Sandbox, extra_args: &[&str]) -> Server {
    let mut args = vec!["server", "--port", "0"];
    args.extend_from_slice(extra_args);
    let mut child = sandbox
        .cmd(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let line = lines
        .next()
        .expect("server exited before announcing its port")
        .unwrap();
    let port = line
        .split("127.0.0.1:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    // Keep draining stdout so the server never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    Server { child, port }
}

/// A registered project whose fake VM is running with an IP.
fn registered_sandbox() -> Sandbox {
    let s = Sandbox::new();
    s.project(
        "[vm]\nname='demo'\n[ssh]\nuser='devbox'\nconfig_path='~/.ssh/config'\nidentity_file='~/key'\n",
    );
    std::fs::write(
        s.project.join(".env"),
        "API_TOKEN=sentinel-private-value\nPLAIN=ok\n",
    )
    .unwrap();
    s.vm("demo", "running", Some("192.168.64.2"));
    let scan = s.run(&["--json", "registry", "scan", s.project.to_str().unwrap()]);
    assert!(scan.status.success());
    s
}

fn get(port: u16, path: &str) -> Value {
    ureq::get(&format!("http://127.0.0.1:{port}{path}"))
        .call()
        .unwrap()
        .into_json()
        .unwrap()
}

fn post(port: u16, path: &str) -> (u16, Value) {
    match ureq::post(&format!("http://127.0.0.1:{port}{path}")).call() {
        Ok(resp) => (resp.status(), resp.into_json().unwrap()),
        Err(ureq::Error::Status(code, resp)) => (code, resp.into_json().unwrap()),
        Err(e) => panic!("{e}"),
    }
}

fn await_job(port: u16, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let job = get(port, &format!("/jobs/{id}"));
        match job["result"]["state"].as_str().unwrap() {
            "running" => {
                assert!(Instant::now() < deadline, "job {id} never finished: {job}");
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => return job,
        }
    }
}

#[test]
fn read_endpoints_serve_registered_vms_with_masked_env() {
    let s = registered_sandbox();
    let server = start_server(&s, &[]);

    let vms = get(server.port, "/vms");
    assert_eq!(vms["result"][0]["name"], "demo");
    assert_eq!(vms["result"][0]["state"], "running");
    assert_eq!(vms["result"][0]["ip"], "192.168.64.2");

    let vm = get(server.port, "/vms/demo");
    assert_eq!(vm["result"]["state"], "running");

    let env = get(server.port, "/vms/demo/env");
    assert_eq!(env["result"]["masked"], true);
    assert_eq!(env["result"]["effective"][0]["value"], "********");
    assert!(
        !env.to_string().contains("sentinel-private-value"),
        "secret leaked; the server must have no reveal path"
    );

    for missing in ["/vms/ghost", "/vms/ghost/env", "/jobs/j99"] {
        let err = ureq::get(&format!("http://127.0.0.1:{}{missing}", server.port))
            .call()
            .unwrap_err();
        match err {
            ureq::Error::Status(404, resp) => {
                let body: Value = resp.into_json().unwrap();
                assert_eq!(body["ok"], false);
            }
            other => panic!("expected 404 for {missing}: {other}"),
        }
    }
}

#[test]
fn lifecycle_actions_run_as_jobs() {
    let s = registered_sandbox();
    s.vm("demo", "stopped", Some("192.168.64.2"));
    let server = start_server(&s, &[]);

    let (status, accepted) = post(server.port, "/vms/demo/start");
    assert_eq!(status, 202);
    let id = accepted["result"]["id"].as_str().unwrap().to_string();
    let done = await_job(server.port, &id);
    assert_eq!(done["result"]["state"], "done");
    assert_eq!(done["result"]["result"]["already_running"], false);
    assert_eq!(get(server.port, "/vms/demo")["result"]["state"], "running");

    let (_, accepted) = post(server.port, "/vms/demo/provision");
    let id = accepted["result"]["id"].as_str().unwrap().to_string();
    let done = await_job(server.port, &id);
    assert_eq!(done["result"]["state"], "done", "{done}");
    let payload =
        std::fs::read_to_string(s.world.join("vm-files/etc/profile.d/tart-provision.sh")).unwrap();
    assert!(payload.contains("export API_TOKEN=sentinel-private-value"));
    assert!(
        !done.to_string().contains("sentinel-private-value"),
        "job report leaked a secret"
    );

    let (_, accepted) = post(server.port, "/vms/demo/stop");
    let id = accepted["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(await_job(server.port, &id)["result"]["state"], "done");
    assert_eq!(get(server.port, "/vms/demo")["result"]["state"], "stopped");
}

#[test]
fn concurrent_mutations_on_one_vm_conflict() {
    let s = registered_sandbox();
    // Running without an IP: `start` polls for the IP for its full 60 s wait,
    // which keeps the job busy for the duration of this test.
    s.vm("demo", "running", None);
    let server = start_server(&s, &[]);

    let (status, first) = post(server.port, "/vms/demo/start");
    assert_eq!(status, 202);
    assert_eq!(first["result"]["state"], "running");

    let (status, second) = post(server.port, "/vms/demo/stop");
    assert_eq!(status, 409, "{second}");
    assert_eq!(second["ok"], false);
}

#[test]
fn browser_origins_and_foreign_hosts_are_rejected() {
    let s = registered_sandbox();
    let server = start_server(&s, &["--allow-origin", "http://localhost:5173"]);

    let evil = ureq::get(&format!("http://127.0.0.1:{}/vms", server.port))
        .set("Origin", "http://evil.example")
        .call()
        .unwrap_err();
    assert!(matches!(evil, ureq::Error::Status(403, _)));

    let allowed = ureq::get(&format!("http://127.0.0.1:{}/vms", server.port))
        .set("Origin", "http://localhost:5173")
        .call()
        .unwrap();
    assert_eq!(allowed.status(), 200);
    assert_eq!(
        allowed.header("access-control-allow-origin"),
        Some("http://localhost:5173"),
        "browsers cannot read responses without the CORS header"
    );

    let preflight = ureq::request("OPTIONS", &format!("http://127.0.0.1:{}/vms", server.port))
        .set("Origin", "http://localhost:5173")
        .set("Access-Control-Request-Method", "POST")
        .call()
        .unwrap();
    assert_eq!(preflight.status(), 204);
    assert_eq!(
        preflight.header("access-control-allow-methods"),
        Some("GET, POST")
    );

    // DNS-rebinding guard: a non-local Host is rejected even over loopback.
    let mut raw = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    write!(
        raw,
        "GET /vms HTTP/1.1\r\nHost: attacker.example\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    raw.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "foreign Host accepted: {response}"
    );
}

#[test]
fn shell_streams_terminal_io_bidirectionally() {
    let s = registered_sandbox();
    let server = start_server(&s, &[]);

    let (mut ws, _) =
        tungstenite::connect(format!("ws://127.0.0.1:{}/vms/demo/shell", server.port)).unwrap();
    // Resize control message must be accepted without killing the session.
    ws.send(Message::Text(
        r#"{"type":"resize","cols":120,"rows":40}"#.into(),
    ))
    .unwrap();
    ws.send(Message::Binary(b"hello\r".to_vec().into()))
        .unwrap();

    let mut transcript = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !transcript.contains("echo:hello") {
        assert!(Instant::now() < deadline, "no echo in: {transcript:?}");
        match ws.read().unwrap() {
            Message::Binary(bytes) => transcript.push_str(&String::from_utf8_lossy(&bytes)),
            Message::Text(text) => panic!("unexpected control message {text} in {transcript:?}"),
            _ => {}
        }
    }
    assert!(transcript.contains("[fake ssh] interactive session"));
    ws.close(None).unwrap();
}

#[test]
fn shell_requires_a_running_vm() {
    let s = registered_sandbox();
    s.vm("demo", "stopped", Some("192.168.64.2"));
    let server = start_server(&s, &[]);

    let err =
        tungstenite::connect(format!("ws://127.0.0.1:{}/vms/demo/shell", server.port)).unwrap_err();
    match err {
        tungstenite::Error::Http(response) => assert_eq!(response.status(), 409),
        other => panic!("expected HTTP 409 handshake rejection: {other}"),
    }
}
