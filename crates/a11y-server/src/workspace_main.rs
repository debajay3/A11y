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
        Arc, Mutex,
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
fn auth_routes(req: &mut tiny_http::Request, auth: &mut a11y_auth::Store) -> Option<(u16, Result<serde_json::Value, String>)> {
    let route = req.url().to_string();
    if !route.starts_with("/v1/auth/") && !route.starts_with("/v1/admin/") {
        return None;
    }
    let token = req.headers().iter().find(|h| h.field.equiv("Authorization")).and_then(|h| h.value.as_str().strip_prefix("Bearer ")).unwrap_or("").to_string();
    let method = req.method().clone();
    let source = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let result = (|| {
        if route.starts_with("/v1/admin/") {
            auth.admin(&token)?;
        }
        let body = if method == Method::Post {
            let mut bytes = Vec::new();
            req.as_reader().take(8193).read_to_end(&mut bytes).map_err(|_| "Cannot read request")?;
            if bytes.len() > 8192 {
                return Err("Request too large".into());
            }
            serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| "Invalid request")?
        } else {
            serde_json::json!({})
        };
        let get = |key: &str| body.get(key).and_then(|v| v.as_str()).unwrap_or("");
        let value = match (method, route.as_str()) {
            (Method::Post, "/v1/auth/login") => serde_json::to_value(auth.login(get("username"), get("password"), &source)?),
            (Method::Get, "/v1/auth/me") => serde_json::to_value(auth.authenticate(&token)?),
            (Method::Post, "/v1/auth/password") => serde_json::to_value(auth.change_password(&token, get("current_password"), get("password"))?),
            (Method::Post, "/v1/auth/logout") => {
                auth.logout(&token)?;
                Ok(serde_json::json!({"ok":true}))
            }
            (Method::Get, "/v1/admin/users") => serde_json::to_value(auth.list(&token)?),
            (Method::Get, "/v1/admin/grants") => serde_json::to_value(auth.grants(&token)?),
            (Method::Post, "/v1/admin/users") => {
                serde_json::to_value(auth.create(&token, get("username"), get("password"), body["admin"].as_bool().unwrap_or(false), get("access"))?)
            }
            (Method::Post, "/v1/admin/update") => serde_json::to_value(auth.update(
                &token,
                get("id"),
                body["active"].as_bool().ok_or("Active status required")?,
                body["admin"].as_bool().ok_or("Admin status required")?,
                get("access"),
            )?),
            (Method::Post, "/v1/admin/reset") => {
                auth.reset(&token, get("id"), get("password"))?;
                Ok(serde_json::json!({"ok":true}))
            }
            (Method::Post, "/v1/admin/grants") => {
                let g = serde_json::from_value(body).map_err(|_| "Invalid grant")?;
                auth.grant(&token, &g)?;
                Ok(serde_json::json!({"ok":true}))
            }
            _ => return Err("Route not found".into()),
        };
        value.map_err(|_| "Cannot encode response".into())
    })();
    Some((if result.is_ok() { 200 } else { 403 }, result))
}
fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        return Err("Usage: a11y-library-server WORKSPACE_DIR [BIND_ADDRESS:PORT]\nExample: a11y-library-server ./workspace 0.0.0.0:49180".into());
    };
    if root == "--bootstrap-admin" {
        let dir = PathBuf::from(args.next().ok_or("Workspace directory required")?);
        let name = args.next().ok_or("Administrator username required")?;
        if args.next().is_some() {
            return Err("Password must be provided on stdin, not in command arguments".into());
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        let mut password = String::new();
        std::io::stdin().take(257).read_to_string(&mut password).map_err(|e| e.to_string())?;
        a11y_auth::Store::open(&dir.join("accounts.sqlite"))?.bootstrap(&name, password.trim_end())?;
        println!("Administrator created. Change the temporary password at first login.");
        return Ok(());
    }
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
    if !root.join("accounts.sqlite").is_file() {
        return Err("Run --bootstrap-admin WORKSPACE_DIR USERNAME first, with the temporary password on stdin".into());
    }
    let accounts = Arc::new(Mutex::new(a11y_auth::Store::open(&root.join("accounts.sqlite"))?));
    let mut store = WorkspaceStore::open(&root)?;
    let http = if root.join("server-cert.pem").is_file() {
        let ssl = tiny_http::SslConfig {
            certificate: std::fs::read(root.join("server-cert.pem")).map_err(|e| e.to_string())?,
            private_key: std::fs::read(root.join("server-key.pem")).map_err(|e| e.to_string())?,
        };
        tiny_http::Server::https(&bind, ssl).map_err(|e| e.to_string())?
    } else {
        if !bind.starts_with("127.0.0.1:") && !bind.starts_with("[::1]:") {
            return Err("TLS certificate required for network login".into());
        }
        tiny_http::Server::http(&bind).map_err(|e| e.to_string())?
    };
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
    println!("Individual account login required. Legacy workspace keys are not accepted.");
    println!("Keep this Mac awake. Control-C saves and closes every hosted file.");
    let mut hosts: HashMap<String, Hosted> = HashMap::new();
    while !stop.load(Ordering::Relaxed) {
        let Some(mut req) = http.recv_timeout(Duration::from_millis(200)).map_err(|e| e.to_string())? else { continue };
        let auth_header = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str()).unwrap_or("");
        let token = auth_header.strip_prefix("Bearer ").unwrap_or("").to_string();
        let endpoint = {
            let mut db = accounts.lock().map_err(|_| "Accounts unavailable")?;
            auth_routes(&mut req, &mut db)
        };
        let ready = accounts.lock().map_err(|_| "Accounts unavailable")?.ready(&token).is_ok();
        let (status, result) = if let Some(answer) = endpoint {
            answer
        } else if !ready {
            (401, Err("Sign in with your A11y account".to_string()))
        } else {
            let route = req.url().to_string();
            match (req.method(), route.as_str()) {
                (&Method::Get, "/v1/files") => {
                    let mut listing = store.listing();
                    let db = accounts.lock().map_err(|_| "Accounts unavailable")?;
                    listing.files.retain(|f| db.access(&token, &f.id, &f.project).is_ok_and(|r| r != "none"));
                    (200, serde_json::to_value(listing).map_err(|e| e.to_string()))
                }
                (&Method::Post, "/v1/files") => {
                    let result = (|| {
                        let mut bytes = Vec::new();
                        req.as_reader().take((MAX_UPLOAD + 1024 * 1024 + 1) as u64).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                        if bytes.len() > MAX_UPLOAD + 1024 * 1024 {
                            return Err("Upload too large".into());
                        }
                        let mut input: PublishFile = serde_json::from_slice(&bytes).map_err(|_| "Invalid publish request".to_string())?;
                        // Authorize the exact project name the store will persist.
                        // Blank input is the default project, not an unrestricted resource.
                        input.project = if input.project.trim().is_empty() { "Shared project".into() } else { input.project.trim().into() };
                        let db = accounts.lock().map_err(|_| "Accounts unavailable")?;
                        if db.access(&token, "", input.project.trim())? != "editor" {
                            return Err("Editor access to this project is required".into());
                        }
                        let file = store.publish(input)?;
                        serde_json::to_value(file).map_err(|e| e.to_string())
                    })();
                    (201, result)
                }
                (&Method::Post, _) if route.starts_with("/v1/files/") && route.ends_with("/open") => {
                    let id = route.trim_start_matches("/v1/files/").trim_end_matches("/open");
                    let result = (|| {
                        let file = store.catalog.files.iter().find(|f| f.id == id).ok_or("File not found")?;
                        let access = accounts.lock().map_err(|_| "Accounts unavailable")?.access(&token, id, &file.project)?;
                        if access == "none" {
                            return Err("File not found or access denied".into());
                        }
                        let project = file.project.clone();
                        let path = store.path(id)?;
                        if hosts.get(id).is_some_and(|h| h.thread.as_ref().is_none_or(|t| t.is_finished())) {
                            hosts.remove(id);
                        }
                        if !hosts.contains_key(id) {
                            let mut cfg = Config::new(path);
                            let db = accounts.clone();
                            let file_id = id.to_string();
                            cfg.access_check =
                                Some(a11y_server::AccessCheck(Arc::new(move |token| match db.lock().ok()?.access(token, &file_id, &project).ok()?.as_str() {
                                    "editor" => Some(a11y_collab::Role::Editor),
                                    "viewer" => Some(a11y_collab::Role::Viewer),
                                    _ => None,
                                })));
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
