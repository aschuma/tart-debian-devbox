//! Local HTTP/WebSocket control server (`tctl server`).
//!
//! Security model, agreed in the phase-2 design session: the listener binds to
//! `127.0.0.1` only and runs **without authentication** — any local process has
//! full VM control. Browser callers must present an exactly allowlisted
//! `Origin`; all requests must carry a local `Host` (DNS-rebinding guard).
//! Environment previews stay masked; terminal streams are never logged.

mod shell;

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

use crate::backend::{ProcessBackend, TartBackend, VmState};
use crate::config::{self, Config};
use crate::ops::{self, Collected, Ctx};
use crate::registry::{Registry, VmRecord};
use crate::{Error, Result};

pub struct ServerOptions {
    /// Port on `127.0.0.1` (`0` = ephemeral; the bound port is printed).
    pub port: u16,
    /// Exact `Origin` values allowed for browser clients.
    pub allowed_origins: Vec<String>,
}

/// Checks one `--allow-origin` value: exact `scheme://host[:port]` only.
pub fn validate_origin(origin: &str) -> std::result::Result<(), String> {
    let host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    match host {
        Some(h) if !h.is_empty() && !h.contains('*') && !h.contains('/') => Ok(()),
        _ => Err(format!(
            "invalid origin '{origin}': expected exact scheme://host[:port], e.g. http://localhost:5173 (no wildcard, no path)"
        )),
    }
}

/// Runs the server in the foreground until the process is terminated.
pub fn run(opts: ServerOptions) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve(opts))
}

async fn serve(opts: ServerOptions) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", opts.port)).await?;
    let addr = listener.local_addr()?;
    {
        use std::io::Write;
        let mut out = std::io::stdout();
        writeln!(
            out,
            "tctl server listening on http://{addr} — loopback only, no authentication"
        )?;
        out.flush()?;
    }
    let state = AppState {
        backend: Arc::new(ProcessBackend),
        jobs: Arc::new(Jobs::default()),
        shells: Arc::new(shell::ShellLimits::default()),
        allowed_origins: Arc::new(opts.allowed_origins),
    };
    let app = router(state);
    axum::serve(listener, app)
        .await
        .map_err(|e| Error::Backend(format!("server failed: {e}")))?;
    Ok(())
}

fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/vms", get(list_vms))
        .route("/vms/{name}", get(get_vm))
        .route("/vms/{name}/env", get(get_env))
        .route("/vms/{name}/start", post(post_start))
        .route("/vms/{name}/stop", post(post_stop))
        .route("/vms/{name}/provision", post(post_provision))
        .route("/vms/{name}/shell", get(shell::shell))
        .route("/jobs/{id}", get(get_job))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub backend: Arc<dyn TartBackend + Send + Sync>,
    pub jobs: Arc<Jobs>,
    pub shells: Arc<shell::ShellLimits>,
    pub allowed_origins: Arc<Vec<String>>,
}

// ─── Request guard ───────────────────────────────────────────────────────────

/// Validates `Host` (DNS-rebinding guard) and `Origin` (browser allowlist).
/// Allowlisted origins receive the CORS headers browsers need to read the
/// response; their preflight `OPTIONS` is answered directly.
async fn guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !host_is_local(host) {
        return ApiError::new(StatusCode::FORBIDDEN, "Host header is not local").into_response();
    }
    let origin = match req.headers().get(header::ORIGIN) {
        None => return next.run(req).await, // non-browser client
        Some(value) => value.to_str().unwrap_or("").to_owned(),
    };
    if !state.allowed_origins.contains(&origin) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "browser origin not allowlisted (start the server with --allow-origin)",
        )
        .into_response();
    }
    let mut response = if req.method() == axum::http::Method::OPTIONS {
        let mut preflight = StatusCode::NO_CONTENT.into_response();
        preflight.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST"),
        );
        preflight.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        preflight
    } else {
        next.run(req).await
    };
    if let Ok(value) = HeaderValue::from_str(&origin) {
        let headers = response.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

/// `Host` must name this machine's loopback; the port is irrelevant.
fn host_is_local(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

// ─── Error / envelope helpers ────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::VmNotRunning(_) => Self::conflict(e.to_string()),
            other => Self::internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "ok": false, "error": self.message })),
        )
            .into_response()
    }
}

fn envelope<T: Serialize>(result: &T, messages: &[(String, String)]) -> Json<Value> {
    Json(json!({ "ok": true, "result": result, "messages": messages }))
}

type ApiResult = std::result::Result<Response, ApiError>;

async fn blocking<T, F>(f: F) -> std::result::Result<T, ApiError>
where
    F: FnOnce() -> std::result::Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(format!("worker failed: {e}")))?
}

/// The registry row for `name`, or 404. Only registered VMs are addressable.
pub(crate) fn lookup(name: &str) -> std::result::Result<VmRecord, ApiError> {
    if !config::valid_vm_name(name) {
        return Err(ApiError::not_found(format!("invalid VM name '{name}'")));
    }
    Registry::open_default()
        .map_err(ApiError::from)?
        .get(name)
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "'{name}' is not registered (run 'tctl init' or 'tctl registry scan')"
            ))
        })
}

/// The project config behind a registry row, re-checked against the VM name.
pub(crate) fn project_config(row: &VmRecord, name: &str) -> std::result::Result<Config, ApiError> {
    let cfg = Config::load(&row.project_dir).map_err(ApiError::from)?;
    if cfg.vm_name != name {
        return Err(ApiError::conflict(format!(
            "project '{}' now names VM '{}'; re-run 'tctl registry scan'",
            row.project_dir.display(),
            cfg.vm_name
        )));
    }
    Ok(cfg)
}

// ─── Read endpoints ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct VmView {
    name: String,
    state: String,
    ip: Option<String>,
    project_dir: std::path::PathBuf,
    share_dir: Option<std::path::PathBuf>,
    last_provision_at: Option<String>,
    updated_at: String,
}

fn vm_view(
    backend: &dyn TartBackend,
    row: VmRecord,
    live: &HashMap<String, VmState>,
) -> std::result::Result<VmView, ApiError> {
    let state = live.get(&row.name).copied();
    let ip = if state == Some(VmState::Running) {
        backend.ip(&row.name).map_err(ApiError::from)?
    } else {
        None
    };
    Ok(VmView {
        state: match state {
            Some(VmState::Running) => "running".into(),
            Some(VmState::Stopped) => "stopped".into(),
            Some(VmState::Other) => "other".into(),
            None => "unknown".into(),
        },
        ip,
        name: row.name,
        project_dir: row.project_dir,
        share_dir: row.share_dir,
        last_provision_at: row.last_provision_at,
        updated_at: row.updated_at,
    })
}

fn live_states(
    backend: &dyn TartBackend,
) -> std::result::Result<HashMap<String, VmState>, ApiError> {
    Ok(backend
        .list()
        .map_err(ApiError::from)?
        .into_iter()
        .filter(|v| v.source == "local")
        .map(|v| (v.name, v.state))
        .collect())
}

async fn list_vms(State(state): State<AppState>) -> ApiResult {
    let backend = state.backend.clone();
    let views = blocking(move || {
        let live = live_states(&*backend)?;
        let rows = Registry::open_default()
            .map_err(ApiError::from)?
            .list()
            .map_err(ApiError::from)?;
        rows.into_iter()
            .map(|row| vm_view(&*backend, row, &live))
            .collect::<std::result::Result<Vec<_>, _>>()
    })
    .await?;
    Ok(envelope(&views, &[]).into_response())
}

async fn get_vm(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult {
    let backend = state.backend.clone();
    let view = blocking(move || {
        let row = lookup(&name)?;
        let live = live_states(&*backend)?;
        vm_view(&*backend, row, &live)
    })
    .await?;
    Ok(envelope(&view, &[]).into_response())
}

/// Masked environment preview; the server has no secret-reveal path.
async fn get_env(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult {
    let backend = state.backend.clone();
    let (report, messages) = blocking(move || {
        let row = lookup(&name)?;
        let cfg = project_config(&row, &name)?;
        let registry = Registry::open_default().ok();
        let mut collected = Collected::default();
        let mut ctx = Ctx {
            cfg: &cfg,
            backend: &*backend,
            registry: registry.as_ref(),
            reporter: &mut collected,
        };
        let report = ops::env(&mut ctx, false).map_err(ApiError::from)?;
        Ok((report, collected.messages))
    })
    .await?;
    Ok(envelope(&report, &messages).into_response())
}

// ─── Jobs ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Action {
    Start,
    Stop,
    Provision,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Provision => "provision",
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct JobView {
    id: String,
    vm: String,
    action: &'static str,
    /// "running" | "done" | "failed"
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    messages: Vec<(String, String)>,
}

/// In-memory job table. One mutating job per VM; history lives for the
/// server's lifetime (a restart forgets jobs but never stops VMs).
#[derive(Default)]
pub(crate) struct Jobs {
    seq: AtomicU64,
    inner: Mutex<JobsInner>,
}

#[derive(Default)]
struct JobsInner {
    jobs: HashMap<String, JobView>,
    busy: HashSet<String>,
}

impl Jobs {
    fn begin(&self, vm: &str, action: Action) -> std::result::Result<JobView, ApiError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.busy.insert(vm.to_string()) {
            return Err(ApiError::conflict(format!(
                "a job is already running for VM '{vm}'"
            )));
        }
        let id = format!("j{}", self.seq.fetch_add(1, Ordering::Relaxed) + 1);
        let job = JobView {
            id: id.clone(),
            vm: vm.to_string(),
            action: action.name(),
            state: "running",
            result: None,
            error: None,
            messages: Vec::new(),
        };
        inner.jobs.insert(id, job.clone());
        Ok(job)
    }

    fn finish(
        &self,
        id: &str,
        vm: &str,
        outcome: std::result::Result<(Value, Vec<(String, String)>), String>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.busy.remove(vm);
        if let Some(job) = inner.jobs.get_mut(id) {
            match outcome {
                Ok((result, messages)) => {
                    job.state = "done";
                    job.result = Some(result);
                    job.messages = messages;
                }
                Err(error) => {
                    job.state = "failed";
                    job.error = Some(error);
                }
            }
        }
    }

    fn get(&self, id: &str) -> Option<JobView> {
        self.inner.lock().unwrap().jobs.get(id).cloned()
    }
}

fn run_action(
    backend: &dyn TartBackend,
    name: &str,
    action: Action,
) -> std::result::Result<(Value, Vec<(String, String)>), String> {
    let work = || -> std::result::Result<_, Error> {
        let registry = Registry::open_default()?;
        let row = registry
            .get(name)?
            .ok_or_else(|| Error::Config(format!("'{name}' is no longer registered")))?;
        let cfg = Config::load(&row.project_dir)?;
        if cfg.vm_name != name {
            return Err(Error::Config(format!(
                "project now names VM '{}'",
                cfg.vm_name
            )));
        }
        let mut collected = Collected::default();
        let mut ctx = Ctx {
            cfg: &cfg,
            backend,
            registry: Some(&registry),
            reporter: &mut collected,
        };
        let result = match action {
            Action::Start => serde_json::to_value(ops::start(&mut ctx, 60)?)?,
            Action::Stop => serde_json::to_value(ops::stop(&mut ctx)?)?,
            Action::Provision => serde_json::to_value(ops::provision(&mut ctx)?)?,
        };
        Ok((result, collected.messages))
    };
    match std::panic::catch_unwind(AssertUnwindSafe(work)) {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err("job panicked".to_string()),
    }
}

async fn post_action(state: AppState, name: String, action: Action) -> ApiResult {
    let job = {
        let (state, name) = (state.clone(), name.clone());
        blocking(move || {
            lookup(&name)?;
            state.jobs.begin(&name, action)
        })
        .await?
    };
    let id = job.id.clone();
    let worker_state = state.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = run_action(&*worker_state.backend, &name, action);
        worker_state.jobs.finish(&id, &name, outcome);
    });
    Ok((StatusCode::ACCEPTED, envelope(&job, &[])).into_response())
}

async fn post_start(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult {
    post_action(state, name, Action::Start).await
}

async fn post_stop(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult {
    post_action(state, name, Action::Stop).await
}

async fn post_provision(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult {
    post_action(state, name, Action::Provision).await
}

async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let job = state
        .jobs
        .get(&id)
        .ok_or_else(|| ApiError::not_found(format!("no job '{id}'")))?;
    Ok(envelope(&job, &[]).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_must_be_exact() {
        assert!(validate_origin("http://localhost:5173").is_ok());
        assert!(validate_origin("https://ui.example").is_ok());
        for bad in [
            "localhost:5173",
            "http://*",
            "http://",
            "http://ui.example/",
            "ftp://x",
            "*",
        ] {
            assert!(validate_origin(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn host_guard_accepts_only_loopback_names() {
        for good in [
            "127.0.0.1:7878",
            "localhost:7878",
            "localhost",
            "[::1]:7878",
        ] {
            assert!(host_is_local(good), "rejected {good:?}");
        }
        for bad in ["attacker.example:7878", "192.168.1.5:7878", "", "[::2]:1"] {
            assert!(!host_is_local(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn one_mutating_job_per_vm() {
        let jobs = Jobs::default();
        let first = jobs.begin("demo", Action::Start).unwrap();
        assert!(jobs.begin("demo", Action::Stop).is_err());
        assert!(jobs.begin("other", Action::Stop).is_ok());
        jobs.finish(&first.id, "demo", Ok((Value::Null, vec![])));
        assert!(jobs.begin("demo", Action::Stop).is_ok());
        assert_eq!(jobs.get(&first.id).unwrap().state, "done");
    }
}
