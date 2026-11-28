//! Shared file library. Network requests run off the UI thread; cached entries never become local copies.
use crate::{app::App, home::Section, theme};
use a11y_library::workspace::{Catalog, OpenFile, PublishFile, MAX_UPLOAD};
use egui::{Context, Rect, Ui};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[derive(Clone, Default, Serialize, Deserialize)]
struct Connection {
    url: String,
    key: String,
}
enum Reply {
    Listed(Catalog),
    Published(Catalog),
    Opened(String),
}
#[derive(Default)]
pub struct Workspace {
    settings_root: Option<PathBuf>,
    connection: Connection,
    pub catalog: Catalog,
    pub draft: Option<String>,
    project: String,
    status: String,
    error: Option<String>,
    receiver: Option<Receiver<Result<Reply, String>>>,
    next_refresh: f64,
    connected: bool,
    managed: bool,
}
impl Workspace {
    pub fn use_session(&mut self, url: &str, token: &str) {
        self.connection = Connection { url: url.into(), key: token.into() };
        self.managed = true;
        self.next_refresh = 0.;
        self.receiver = None;
        self.catalog = Catalog::default();
    }
    pub fn load(&mut self, root: &Path) {
        self.settings_root = Some(root.to_path_buf());
        if self.managed {
            self.catalog = Catalog::default();
            self.next_refresh = 0.;
            self.receiver = None;
            return;
        }
        self.connection = std::fs::read(root.join("a11y-workspace.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        self.catalog = std::fs::read(root.join("a11y-shared-cache.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if self.connection.url.is_empty() {
            self.connection.url = "http://127.0.0.1:49180".into();
        }
        self.project = "Shared project".into();
    }
    fn sign_out(&mut self) -> Result<(), String> {
        if let Some(root) = &self.settings_root {
            save_private(root, "a11y-shared-cache.json", &Catalog::default())?;
            save_private(root, "a11y-workspace.json", &Connection { url: self.connection.url.clone(), key: String::new() })?;
        }
        let root = self.settings_root.clone();
        let url = self.connection.url.clone();
        *self = Self::default(); // Drops pending replies so an old request cannot restore the session.
        self.settings_root = root;
        self.connection.url = url;
        Ok(())
    }

    pub fn configured(&self) -> bool {
        !self.connection.key.is_empty()
    }
    fn start(&mut self, ctx: &Context, task: impl FnOnce(Connection) -> Result<Reply, String> + Send + 'static) {
        if self.receiver.is_some() {
            return;
        }
        self.error = None;
        let conn = self.connection.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(task(conn));
            ctx.request_repaint();
        });
    }
}
fn request<T: serde::de::DeserializeOwned>(c: &Connection, method: &str, path: &str, body: Option<String>) -> Result<T, String> {
    let url = url::Url::parse(c.url.trim()).map_err(|_| "Enter a server address, for example http://192.168.1.10:49180".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Use the server's http:// or https:// address without a path, password, or query".into());
    }
    let agent = crate::accounts::agent()?;
    let req = agent.request(method, &format!("{}{}", c.url.trim().trim_end_matches('/'), path)).set("Authorization", &format!("Bearer {}", c.key.trim()));
    let result = if let Some(body) = body { req.set("Content-Type", "application/json").send_string(&body) } else { req.call() };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(401, _)) => return Err("Your account session expired. Sign out and sign in again.".into()),
        Err(ureq::Error::Status(_, r)) => {
            let mut text = String::new();
            let _ = r.into_reader().take(8192).read_to_string(&mut text);
            return Err(serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| "The server could not complete this request".into()));
        }
        Err(_) => return Err("Cannot reach the shared library. Check that the server Mac is awake and on the same network.".into()),
    };
    let mut bytes = Vec::new();
    response.into_reader().take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Server response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "The server returned an invalid response".into())
}
fn list(c: &Connection) -> Result<Catalog, String> {
    request(c, "GET", "/v1/files", None)
}
fn save_private(root: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    // Create the temporary credential file with private permissions before writing any key.
    let path = root.join(name);
    let temp = root.join(format!("{name}.tmp"));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&temp).map_err(|e| e.to_string())?;
    use std::io::Write;
    f.write_all(&bytes).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())
}
impl App {
    pub(crate) fn sign_out_workspace(&mut self) {
        if !self.leave_all_ok(|app| app.sign_out_workspace()) {
            return;
        }
        self.go_home();
        if self.home.is_none() {
            return;
        }
        if let Err(error) = self.workspace.sign_out() {
            self.show_toast(format!("Could not sign out: {error}"), true);
            return;
        }
        self.accounts.logout();
        self.end_collab();
        self.cancel_join();
        self.stop_bridge();
        self.desktop_tabs = Default::default();
        self.clip = None;
        self.parked_home = None;
        self.set_author("Guest");
        if let Some(home) = &mut self.home {
            home.section = Section::Shared;
            home.profile_menu = None;
        }
        self.show_toast("Signed out. Reconnect to the shared library to access team files.", false);
    }

    pub fn publish_to_workspace(&mut self, text: String) {
        let again = text.clone();
        if !self.leave_ok(move |app| app.publish_to_workspace(again)) {
            return;
        }
        self.go_home();
        if let Some(h) = &mut self.home {
            h.section = Section::Shared;
        }
        self.workspace.draft = Some(text);
    }
}

// Intent: a designer returning to a team document; calm, familiar and clear about where files live.
// Palette/surfaces: existing graphite text, canvas white, selected blue, border grey and status red.
// Depth: existing thin separators; Inter typography; 8px spacing. Signature: persistent shared file + server status.
// Avoid a second dashboard, duplicate local cards, or a technical session-link list: use one shared library in navigation.
pub fn show(app: &mut App, ui: &mut Ui, rect: Rect, search: &str) -> Option<(String, String)> {
    let ctx = ui.ctx().clone();
    let mut w = std::mem::take(&mut app.workspace);
    let mut opened = None;
    if let Some(rx) = &w.receiver {
        match rx.try_recv() {
            Ok(result) => {
                w.receiver = None;
                w.next_refresh = app.now + 10.0;
                match result {
                    Ok(reply) => {
                        w.connected = true;
                        match reply {
                            Reply::Listed(c) => {
                                w.catalog = c;
                                w.status = "Connected · files saved on the server".into();
                            }
                            Reply::Published(c) => {
                                w.catalog = c;
                                w.draft = None;
                                w.status = "Published. Open the shared file below to collaborate.".into();
                            }
                            Reply::Opened(link) => opened = Some(link),
                        }
                        if let Some(root) = w.settings_root.as_deref().filter(|_| !w.managed) {
                            if let Err(e) =
                                save_private(root, "a11y-workspace.json", &w.connection).and_then(|_| save_private(root, "a11y-shared-cache.json", &w.catalog))
                            {
                                w.error = Some(format!("Connected, but could not remember this workspace: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        w.error = Some(e);
                        w.connected = false;
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                w.receiver = None;
                w.error = Some("The request stopped unexpectedly. Try again.".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    if w.configured() && w.receiver.is_none() && app.now >= w.next_refresh {
        w.next_refresh = app.now + 10.0;
        w.start(&ctx, |c| list(&c).map(Reply::Listed));
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(24.0)), |ui| {
        ui.heading("Shared library");
        ui.label("One place for your team's files. Reopen a file here to continue collaborating.");
        ui.add_space(16.0);
        if app.accounts.session.as_ref().is_some_and(|s| s.user.admin) && ui.button("Admin · People and access").clicked() {
            app.accounts.admin_open = true;
        }
        if w.managed {
            ui.label(format!("Signed in · {}", w.connection.url));
        } else {
            egui::CollapsingHeader::new("Workspace connection").default_open(!w.connected).show(ui, |ui| {
                ui.label("Server address");
                let busy = w.receiver.is_some();
                ui.add_enabled(!busy, egui::TextEdit::singleline(&mut w.connection.url).desired_width(360.0));
                ui.label("Workspace key");
                ui.add_enabled(!busy, egui::TextEdit::singleline(&mut w.connection.key).password(true).desired_width(360.0));
                ui.small("Local demo: everyone with this key can browse, publish and edit. Use a trusted network.");
                if ui.add_enabled(!busy && !w.connection.key.trim().is_empty(), egui::Button::new("Connect and remember")).clicked() {
                    w.connection.url = w.connection.url.trim().trim_end_matches('/').to_string();
                    w.connection.key = w.connection.key.trim().to_string();
                    w.catalog = Catalog::default();
                    w.start(&ctx, |c| list(&c).map(Reply::Listed));
                }
            });
        }
        ui.add_space(8.0);
        if w.receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Contacting shared library…");
            });
        } else if let Some(error) = &w.error {
            ui.colored_label(egui::Color32::from_rgb(190, 50, 55), error);
            if !w.catalog.files.is_empty() {
                ui.small("Showing the last known file list. Opening a file requires the server.");
            }
        } else {
            ui.label(&w.status);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(w.receiver.is_none() && w.configured(), egui::Button::new("Refresh")).clicked() {
                w.start(&ctx, |c| list(&c).map(Reply::Listed));
            }
            if ui.add_enabled(w.receiver.is_none(), egui::Button::new("Publish a file…")).clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Design file", &["a11y", "fig", "pen", "svg", "json"]).pick_file() {
                    match a11y_library::load_external(&path).and_then(|d| d.to_json().map_err(|e| a11y_library::LibraryError::Invalid(e.to_string()))) {
                        Ok(s) => w.draft = Some(s),
                        Err(e) => w.error = Some(e.to_string()),
                    }
                }
            }
        });
        if w.draft.is_some() {
            ui.add_space(8.0);
            ui.group(|ui| {
                ui.label("Publish a shared copy");
                ui.small("The original stays on this Mac. Future shared edits belong to the server copy.");
                ui.horizontal(|ui| {
                    ui.label("Project");
                    ui.text_edit_singleline(&mut w.project);
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(w.connected && w.receiver.is_none(), egui::Button::new("Publish to shared library")).clicked() {
                        let doc = w.draft.clone().unwrap_or_default();
                        let project = w.project.clone();
                        if doc.len() > MAX_UPLOAD {
                            w.error = Some("This file exceeds the 32 MB demo upload limit".into());
                        } else {
                            w.start(&ctx, move |c| {
                                let payload = serde_json::to_string(&PublishFile { project, document: doc }).map_err(|e| e.to_string())?;
                                let _: a11y_library::workspace::SharedFile = request(&c, "POST", "/v1/files", Some(payload))?;
                                list(&c).map(Reply::Published)
                            });
                        }
                    }
                    if ui.add_enabled(w.receiver.is_none(), egui::Button::new("Cancel")).clicked() {
                        w.draft = None;
                    }
                });
            });
        }
        ui.add_space(16.0);
        let query = search.trim().to_lowercase();
        let files: Vec<_> =
            w.catalog.files.iter().filter(|f| query.is_empty() || format!("{} {}", f.name, f.project).to_lowercase().contains(&query)).cloned().collect();
        if files.is_empty() {
            ui.label(if w.catalog.files.is_empty() {
                "No shared files yet. Connect to your workspace, then publish a design."
            } else {
                "No matching shared files."
            });
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for file in files {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&file.name).strong());
                        ui.label(egui::RichText::new(&file.project).color(theme::TEXT_SECONDARY));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(w.receiver.is_none(), egui::Button::new("Open shared file")).clicked() {
                            w.start(&ctx, move |c| {
                                request::<OpenFile>(&c, "POST", &format!("/v1/files/{}/open", file.id), None).map(|r| Reply::Opened(r.link))
                            });
                        }
                    });
                });
                ui.separator();
            }
        });
    });
    let key = w.connection.key.clone();
    app.workspace = w;
    ctx.request_repaint_after(Duration::from_millis(500));
    opened.map(|link| (link, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signing_out_forgets_credentials_and_cached_files_across_restart() {
        let root = std::env::temp_dir().join(format!("a11y-signout-{}", a11y_library::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        save_private(&root, "a11y-workspace.json", &Connection { url: "http://127.0.0.1:49180".into(), key: "test-key".into() }).unwrap();
        let mut workspace = Workspace::default();
        workspace.load(&root);
        let (sender, receiver) = mpsc::channel();
        workspace.receiver = Some(receiver);
        workspace.sign_out().unwrap();
        assert!(!workspace.configured());
        assert!(sender.send(Ok(Reply::Listed(Catalog::default()))).is_err());
        let mut restored = Workspace::default();
        restored.load(&root);
        assert!(!restored.configured());
        assert!(restored.catalog.files.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connection_and_cached_catalog_survive_an_app_restart() {
        let root = std::env::temp_dir().join(format!("a11y-client-{}", a11y_library::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let connection = Connection { url: "http://127.0.0.1:49180".into(), key: "test-only".into() };
        let catalog = Catalog { files: vec![a11y_library::workspace::SharedFile { id: "abc123".into(), name: "Team file".into(), project: "Website".into() }] };
        save_private(&root, "a11y-workspace.json", &connection).unwrap();
        save_private(&root, "a11y-shared-cache.json", &catalog).unwrap();
        let mut restored = Workspace::default();
        restored.load(&root);
        assert!(restored.configured());
        assert_eq!(restored.catalog.files[0].id, "abc123");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(root.join("a11y-workspace.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn organizations_keep_the_shared_connection_across_switch_and_restart() {
        let root = std::env::temp_dir().join(format!("a11y-org-shared-{}", a11y_library::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let connection = Connection { url: "http://127.0.0.1:49180".into(), key: "test-only".into() };
        save_private(&root, "a11y-workspace.json", &connection).unwrap();
        let mut app = App::new(crate::demo::demo_document());
        app.organizations = Some(a11y_library::organizations::Workspace::open(root.clone()).unwrap());
        app.workspace.load(&root);
        app.home = Some(crate::home::Home::new());
        assert!(app.create_organization("Design team"));
        assert!(app.workspace.configured());
        assert_eq!(app.workspace.settings_root.as_deref(), Some(root.as_path()));
        let team_root = app.lib.as_ref().unwrap().root().to_path_buf();
        assert_ne!(team_root, root);
        let mut restarted = App::new(crate::demo::demo_document());
        let active = restarted.workspace_root(root.clone());
        assert_eq!(active, team_root);
        restarted.attach_library(active);
        assert_eq!(restarted.workspace.connection.key, "test-only");
        restarted.switch_organization(None);
        assert_eq!(restarted.lib.as_ref().unwrap().root(), root);
        assert!(restarted.workspace.configured());
        let _ = std::fs::remove_dir_all(root);
    }
}
