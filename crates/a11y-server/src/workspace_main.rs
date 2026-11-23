//! A local-network shared catalog with an independent collaboration host for each open file.
use a11y_library::workspace::{OpenFile, PublishFile, WorkspaceStore, MAX_UPLOAD};
use a11y_net::Network;
use a11y_server::{Config, Server};
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tiny_http::{Header, Method, Response, StatusCode};

struct Hosted {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    link: String,
}
impl Drop for Hosted {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn authorized(actual: &str, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    actual.len() == expected.len() && actual.bytes().zip(expected.bytes()).fold(0u8, |a, (b, c)| a | (b ^ c)) == 0
}
fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        return Err("Usage: a11y-library-server WORKSPACE_DIR [BIND_ADDRESS:PORT]\nExample: a11y-library-server ./workspace 0.0.0.0:49180".into());
    };
    let root = PathBuf::from(root);
    let bind = args.next().unwrap_or_else(|| "127.0.0.1:49180".into());
    if args.next().is_some() {
        return Err("Too many arguments".into());
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join("server.lock")).map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| "This workspace is already running in another server".to_string())?;
    let key = a11y_server::load_or_create_key(&root.join("workspace.key"))?;
    let token: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let mut store = WorkspaceStore::open(&root)?;
    let http = tiny_http::Server::http(&bind).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("signal runtime");
        rt.block_on(async {
            #[cfg(unix)]
            {
                if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
                } else {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        });
        signal.store(true, Ordering::Relaxed);
    });
    println!("A11y shared library listening on {bind}");
    println!("Workspace key: read {} locally and share it only with your demo collaborators.", root.join("workspace.key").display());
    println!("Keep this Mac awake. Control-C saves and closes every hosted file.");
    let mut hosts: HashMap<String, Hosted> = HashMap::new();
    while !stop.load(Ordering::Relaxed) {
        let Some(mut req) = http.recv_timeout(Duration::from_millis(200)).map_err(|e| e.to_string())? else { continue };
        let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str()).unwrap_or("");
        let (status, result) = if !authorized(auth, &token) {
            (401, Err("Workspace key is missing or incorrect".to_string()))
        } else {
            let route = req.url().to_string();
            match (req.method(), route.as_str()) {
                (&Method::Get, "/v1/files") => (200, serde_json::to_value(store.listing()).map_err(|e| e.to_string())),
                (&Method::Post, "/v1/files") => {
                    let result = (|| {
                        let mut bytes = Vec::new();
                        req.as_reader().take((MAX_UPLOAD + 1024 * 1024 + 1) as u64).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                        if bytes.len() > MAX_UPLOAD + 1024 * 1024 {
                            return Err("Upload too large".into());
                        }
                        let input: PublishFile = serde_json::from_slice(&bytes).map_err(|_| "Invalid publish request".to_string())?;
                        let file = store.publish(input)?;
                        serde_json::to_value(file).map_err(|e| e.to_string())
                    })();
                    (201, result)
                }
                (&Method::Post, _) if route.starts_with("/v1/files/") && route.ends_with("/open") => {
                    let id = route.trim_start_matches("/v1/files/").trim_end_matches("/open");
                    let result = (|| {
                        let path = store.path(id)?;
                        if hosts.get(id).is_some_and(|h| h.thread.as_ref().is_none_or(|t| t.is_finished())) {
                            hosts.remove(id);
                        }
                        if !hosts.contains_key(id) {
                            let mut cfg = Config::new(path);
                            cfg.password = Some(token.clone());
                            cfg.network = Network::Direct;
                            cfg.name = format!("A11y {id}");
                            cfg.autosave = Duration::from_secs(1);
                            let index = store.catalog.files.iter().position(|f| f.id == id).ok_or("File missing")?;
                            cfg.port = Some(49200u16.checked_add(u16::try_from(index).map_err(|_| "Workspace full")?).ok_or("Workspace full")?);
                            let mut server = Server::start(cfg)?;
                            let link = server.link().to_string();
                            let quit = Arc::new(AtomicBool::new(false));
                            let q = quit.clone();
                            let thread = std::thread::spawn(move || {
                                server.run(&q);
                                server.shutdown()
                            });
                            hosts.insert(id.into(), Hosted { stop: quit, thread: Some(thread), link });
                        }
                        serde_json::to_value(OpenFile { link: hosts[id].link.clone() }).map_err(|e| e.to_string())
                    })();
                    (200, result)
                }
                _ => (404, Err("Route not found".to_string())),
            }
        };
        let (code, body) = match result {
            Ok(v) => (status, v),
            Err(e) => (if status < 400 { 400 } else { status }, serde_json::json!({"error":e})),
        };
        let response = Response::from_string(body.to_string())
            .with_status_code(StatusCode(code))
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
        let _ = req.respond(response);
    }
    drop(hosts);
    println!("A11y shared library stopped; documents saved.");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("A11y: {e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_requires_exact_key() {
        assert!(authorized("Bearer abc", "abc"));
        assert!(!authorized("Bearer abd", "abc"));
        assert!(!authorized("", "abc"));
        assert!(!authorized("Bearer abc-extra", "abc"));
    }
}
