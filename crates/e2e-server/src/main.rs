// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-e2e-server`: the server behind the UI's end-to-end tests (ADR-0018).
//!
//! It serves the e2e build of the web UI with the app's Content-Security-Policy, and answers the
//! UI's commands by calling the same `dbme-api` functions the desktop shell calls, on real vaults.
//! Each test uses its own session, named by the test, with its own folder and vault. Committed
//! changes travel back with the next reply as `vault-changed` events.
//!
//! Test only: it listens on 127.0.0.1 and is never shipped.
//!
//! ```text
//! dbme-e2e-server --dist apps/desktop/dist-e2e --data target/e2e-data [--port 4310]
//! ```
//!
//! - `GET /...`: a file of the UI (`index.html` for paths without an extension).
//! - `POST /ipc` `{"session", "cmd", "args"}` → `{"ok": value}` or `{"error": IpcError}`, each
//!   with `"events": [Committed, ...]`.
//! - `POST /seed` `{"session", "seed"}` → the IDs it created (see `seed.rs`).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dbme_api::{IpcError, IpcErrorKind, Session};
use dbme_store::Committed;
use serde::Deserialize;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

mod dispatch;
mod seed;

/// The app's window configuration, for its Content-Security-Policy.
const TAURI_CONF: &str = include_str!("../../../apps/desktop/src-tauri/tauri.conf.json");

/// A file that marks a data folder as this server's, so it may empty it.
const MARKER: &str = ".dbme-e2e";

/// The largest request body: base64 of a 32 MiB attachment, with room to spare.
const MAX_BODY: u64 = 48 * 1024 * 1024;

const WORKERS: usize = 8;

type Error = Box<dyn std::error::Error + Send + Sync>;

/// One test's session: its folder, its vault and the changes not yet sent to the UI.
pub(crate) struct TestSession {
    pub(crate) api: Session,
    pub(crate) dir: PathBuf,
    events: Arc<Mutex<Vec<Committed>>>,
    /// Held across a command and the taking of its events, so each reply carries exactly the
    /// changes its own command committed: a test reading the vault never takes the UI's events.
    calls: Mutex<()>,
}

impl TestSession {
    fn new(dir: PathBuf) -> Result<Self, Error> {
        std::fs::create_dir_all(dir.join("vaults"))?;
        let events: Arc<Mutex<Vec<Committed>>> = Arc::default();
        let sink = Arc::clone(&events);
        let api = Session::new(move |committed| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(committed.clone());
        });
        Ok(Self {
            api,
            dir,
            events,
            calls: Mutex::new(()),
        })
    }

    /// The vault `vault_open_default` opens.
    pub(crate) fn default_vault(&self) -> PathBuf {
        self.dir.join("vaults").join("Main.dbme")
    }

    fn take_events(&self) -> Vec<Committed> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

struct State {
    dist: PathBuf,
    data: PathBuf,
    csp: String,
    sessions: Mutex<HashMap<String, Arc<TestSession>>>,
}

impl State {
    fn session(&self, name: &str) -> Result<Arc<TestSession>, Error> {
        if name.is_empty()
            || name.len() > 64
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("bad session name {name:?}").into());
        }
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(session) = sessions.get(name) {
            return Ok(Arc::clone(session));
        }
        let session = Arc::new(TestSession::new(self.data.join(name))?);
        sessions.insert(name.to_owned(), Arc::clone(&session));
        Ok(session)
    }
}

/// The policy from `tauri.conf.json`, with `connect-src` allowing this server: the UI's commands
/// travel over `fetch` here instead of Tauri's IPC.
fn content_security_policy() -> Result<String, Error> {
    let conf: Value = serde_json::from_str(TAURI_CONF)?;
    let csp = conf["app"]["security"]["csp"]
        .as_object()
        .ok_or("tauri.conf.json has no app.security.csp map")?;
    let directives: Vec<String> = csp
        .iter()
        .map(|(name, value)| {
            let value = if name == "connect-src" {
                "'self'"
            } else {
                value.as_str().unwrap_or_default()
            };
            format!("{name} {value}")
        })
        .collect();
    Ok(directives.join("; "))
}

/// Takes the data folder over: creates it, or empties it if an earlier run left it.
fn prepare_data(data: &Path) -> Result<(), Error> {
    if data.exists() {
        let empty = std::fs::read_dir(data)?.next().is_none();
        if !empty && !data.join(MARKER).exists() {
            return Err(format!(
                "{} is not empty and not an e2e data folder; refusing to clear it",
                data.display()
            )
            .into());
        }
        std::fs::remove_dir_all(data)?;
    }
    std::fs::create_dir_all(data)?;
    std::fs::write(
        data.join(MARKER),
        "dbme-e2e-server data; deleted on the next run\n",
    )?;
    Ok(())
}

type Reply = Response<std::io::Cursor<Vec<u8>>>;

/// Adds a header. `from_bytes` refuses only non-ASCII text, and every name and value here is
/// ASCII, including the policy built from `tauri.conf.json`.
fn with(response: Reply, name: &str, value: &str) -> Reply {
    match Header::from_bytes(name.as_bytes(), value.as_bytes()) {
        Ok(header) => response.with_header(header),
        Err(()) => response,
    }
}

fn json_reply(status: u16, body: &Value) -> Reply {
    let response = Response::from_data(body.to_string().into_bytes()).with_status_code(status);
    let response = with(response, "Content-Type", "application/json");
    with(response, "Cache-Control", "no-store")
}

fn text_reply(status: u16, text: &str) -> Reply {
    let response = Response::from_data(text.as_bytes().to_vec()).with_status_code(status);
    with(response, "Content-Type", "text/plain; charset=utf-8")
}

fn read_json<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, Error> {
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)?;
    Ok(serde_json::from_str(&body)?)
}

#[derive(Deserialize)]
struct IpcRequest {
    session: String,
    cmd: String,
    #[serde(default)]
    args: Value,
}

fn ipc(state: &State, request: &mut Request) -> Result<Reply, Error> {
    let call: IpcRequest = read_json(request)?;
    let session = state.session(&call.session)?;
    let (result, events) = {
        let _call = session.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let result = dispatch::dispatch(&session, &call.cmd, &call.args);
        (result, session.take_events())
    };
    let events = serde_json::to_value(events)?;
    Ok(json_reply(
        200,
        &match result {
            Ok(value) => json!({ "ok": value, "events": events }),
            Err(error) => json!({ "error": error, "events": events }),
        },
    ))
}

#[derive(Deserialize)]
struct SeedRequest {
    session: String,
    seed: seed::Seed,
}

fn seed(state: &State, request: &mut Request) -> Result<Reply, Error> {
    let call: SeedRequest = read_json(request)?;
    let session = state.session(&call.session)?;
    let _call = session.calls.lock().unwrap_or_else(PoisonError::into_inner);
    if dbme_api::vault_info(&session.api).is_ok_and(|info| info.is_none()) {
        let path = session.default_vault().display().to_string();
        dispatch::dispatch(&session, "vault_create", &json!({ "path": path }))
            .map_err(|e: IpcError| e.message)?;
    }
    let ids = seed::run(&session.api, call.seed).map_err(|e| e.message)?;
    // Seeding sets the scene before the UI looks; its changes are not news to send it.
    drop(session.take_events());
    Ok(json_reply(200, &ids))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn static_file(state: &State, url_path: &str) -> Reply {
    let relative = url_path.trim_start_matches('/');
    let safe = !relative.contains("..") && !relative.contains('\\') && !relative.contains(':');
    if !safe {
        return text_reply(400, "bad path");
    }
    let mut path = state.dist.join(relative);
    if relative.is_empty() || (Path::new(relative).extension().is_none() && !path.is_file()) {
        path = state.dist.join("index.html");
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut response = with(
                Response::from_data(bytes),
                "Content-Type",
                content_type(&path),
            );
            response = with(response, "Cache-Control", "no-store");
            response = with(response, "X-Content-Type-Options", "nosniff");
            if content_type(&path).starts_with("text/html") {
                response = with(response, "Content-Security-Policy", &state.csp);
            }
            response
        }
        Err(_) => text_reply(404, "not found"),
    }
}

fn handle(state: &State, mut request: Request) {
    let url = request.url().to_owned();
    let path = url.split('?').next().unwrap_or("/").to_owned();
    let response = match (request.method(), path.as_str()) {
        (Method::Post, "/ipc") => ipc(state, &mut request),
        (Method::Post, "/seed") => seed(state, &mut request),
        (Method::Get, "/health") => Ok(text_reply(200, "ok")),
        (Method::Get, _) => Ok(static_file(state, &path)),
        _ => Ok(text_reply(405, "method not allowed")),
    };
    let response = response.unwrap_or_else(|error| {
        let error = IpcError::new(IpcErrorKind::Invalid, error.to_string());
        json_reply(400, &json!({ "error": error, "events": [] }))
    });
    if let Err(error) = request.respond(response) {
        eprintln!("dbme-e2e-server: could not reply to {path}: {error}");
    }
}

struct Args {
    dist: PathBuf,
    data: PathBuf,
    port: u16,
}

fn parse_args() -> Result<Args, Error> {
    let mut dist = None;
    let mut data = None;
    let mut port = 4310;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--dist" => dist = Some(PathBuf::from(value)),
            "--data" => data = Some(PathBuf::from(value)),
            "--port" => port = value.parse()?,
            _ => return Err(format!("unknown flag {flag}").into()),
        }
    }
    Ok(Args {
        dist: dist.ok_or("--dist is required")?,
        data: data.ok_or("--data is required")?,
        port,
    })
}

fn main() -> Result<(), Error> {
    let args = parse_args()?;
    let dist = args.dist.canonicalize()?;
    if !dist.join("index.html").is_file() {
        return Err(format!(
            "{} has no index.html; build the e2e UI first",
            dist.display()
        )
        .into());
    }
    prepare_data(&args.data)?;
    let state = Arc::new(State {
        dist,
        data: args.data.canonicalize()?,
        csp: content_security_policy()?,
        sessions: Mutex::new(HashMap::new()),
    });
    let server = Arc::new(Server::http(("127.0.0.1", args.port))?);
    println!(
        "dbme-e2e-server listening on http://127.0.0.1:{}",
        args.port
    );
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (server, state) = (Arc::clone(&server), Arc::clone(&state));
            std::thread::spawn(move || {
                while let Ok(request) = server.recv() {
                    handle(&state, request);
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_is_the_apps_with_this_server_for_connect() {
        let csp = content_security_policy().unwrap();
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("style-src 'self'"));
        assert!(csp.contains("connect-src 'self'"));
        assert!(!csp.contains("ipc:"));
    }

    #[test]
    fn a_foreign_folder_is_never_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let foreign = dir.path().join("foreign");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("precious.txt"), "keep").unwrap();
        assert!(prepare_data(&foreign).is_err());
        assert!(foreign.join("precious.txt").exists());

        let ours = dir.path().join("ours");
        prepare_data(&ours).unwrap();
        std::fs::write(ours.join("old"), "x").unwrap();
        prepare_data(&ours).unwrap();
        assert!(!ours.join("old").exists() && ours.join(MARKER).exists());
    }

    #[test]
    fn session_names_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let state = State {
            dist: dir.path().to_owned(),
            data: dir.path().to_owned(),
            csp: String::new(),
            sessions: Mutex::new(HashMap::new()),
        };
        assert!(state.session("../up").is_err());
        assert!(state.session("").is_err());
        let a = state.session("test-1").unwrap();
        assert!(Arc::ptr_eq(&a, &state.session("test-1").unwrap()));
    }
}
