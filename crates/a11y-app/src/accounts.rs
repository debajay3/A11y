//! In-app sign-in and administration. Passwords stay in memory; remembered sessions use the OS vault.
//! Intent: designers returning to shared files; admins granting access without leaving their editor.
//! Use the existing Inter/graphite/blue theme, thin borders and 8-point spacing for continuity.
use crate::{app::App, theme};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Read,
    sync::mpsc::{self, Receiver},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub admin: bool,
    pub active: bool,
    pub must_change_password: bool,
    pub access: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user: User,
    pub expires: i64,
}
#[derive(Clone, Deserialize)]
struct Grant {
    user_id: String,
    kind: String,
    resource: String,
    role: String,
}
enum Reply {
    Login(Session),
    Check(User),
    Users(Vec<User>, Vec<Grant>),
    Done,
}
pub struct Accounts {
    pub enabled: bool,
    pub server: String,
    pub session: Option<Session>,
    username: String,
    password: String,
    new_password: String,
    error: String,
    status: String,
    pending: Option<Receiver<Result<Reply, String>>>,
    pub admin_open: bool,
    users: Vec<User>,
    grants: Vec<Grant>,
    new_user: String,
    new_admin: bool,
    new_access: String,
    reset_user: Option<String>,
    reset_password: String,
    pub applied: bool,
    next_check: f64,
    remember: bool,
    grant_user: String,
    grant_kind: String,
    grant_resource: String,
    grant_role: String,
}
impl Default for Accounts {
    fn default() -> Self {
        Self {
            enabled: false,
            server: "https://10.20.25.165:49180".into(),
            session: None,
            username: String::new(),
            password: String::new(),
            new_password: String::new(),
            error: String::new(),
            status: String::new(),
            pending: None,
            admin_open: false,
            users: Vec::new(),
            grants: Vec::new(),
            new_user: String::new(),
            new_admin: false,
            new_access: "none".into(),
            reset_user: None,
            reset_password: String::new(),
            applied: false,
            next_check: 0.,
            remember: true,
            grant_user: String::new(),
            grant_kind: "project".into(),
            grant_resource: String::new(),
            grant_role: "viewer".into(),
        }
    }
}
fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}
fn vault() -> Result<keyring::Entry, String> {
    if cfg!(test) {
        return Err("Credential store disabled in tests".into());
    }
    keyring::Entry::new("A11y accounts", "remembered-session").map_err(|e| e.to_string())
}
pub fn agent() -> Result<ureq::Agent, String> {
    let mut roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    for cert in rustls_pemfile::certs(&mut include_bytes!("../assets/server/local-ca.pem").as_slice()) {
        roots.add(cert.map_err(|_| "Invalid workspace certificate")?).map_err(|_| "Invalid workspace certificate")?;
    }
    let tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    Ok(ureq::AgentBuilder::new().tls_config(std::sync::Arc::new(tls)).redirects(0).timeout(Duration::from_secs(15)).build())
}
pub fn request(server: &str, token: &str, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = url::Url::parse(server).map_err(|_| "Enter a valid HTTPS server address")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Use an HTTPS server address without a path or credentials".into());
    }
    let req = agent()?.request(method, &format!("{}{}", server.trim_end_matches('/'), path)).set("Authorization", &format!("Bearer {token}"));
    let response = match body {
        Some(v) => req.set("Content-Type", "application/json").send_string(&v.to_string()),
        None => req.call(),
    };
    let (failed, response) = match response {
        Ok(r) => (false, r),
        Err(ureq::Error::Status(_, r)) => (true, r),
        Err(_) => return Err("Cannot reach the server securely. Check its address, your network and that the server Mac is awake.".into()),
    };
    let mut bytes = Vec::new();
    response.into_reader().take(1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "Cannot read server response")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Server response too large".into());
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid server response")?;
    if failed {
        Err(v["error"].as_str().unwrap_or("Request denied").into())
    } else {
        Ok(v)
    }
}
impl Accounts {
    pub fn load(&mut self) {
        self.enabled = true;
        if let Ok(entry) = vault() {
            if let Ok(value) = entry.get_password() {
                if let Ok((server, session)) = serde_json::from_str::<(String, Session)>(&value) {
                    self.server = server;
                    if session.expires > now() && !session.user.must_change_password {
                        self.session = Some(session);
                    }
                }
            }
        }
    }
    fn save(&mut self) {
        if !self.remember {
            return;
        }
        if let Some(s) = self.session.as_ref().filter(|s| !s.user.must_change_password) {
            let saved =
                vault().and_then(|v| v.set_password(&serde_json::to_string(&(self.server.clone(), s)).map_err(|e| e.to_string())?).map_err(|e| e.to_string()));
            if saved.is_err() {
                self.status = "Signed in for this session. The system credential store could not remember your login.".into();
            }
        }
    }
    pub fn logout(&mut self) {
        if let Some(s) = self.session.take() {
            let server = self.server.clone();
            std::thread::spawn(move || {
                let _ = request(&server, &s.token, "POST", "/v1/auth/logout", Some(json!({})));
            });
        }
        if let Ok(v) = vault() {
            let _ = v.delete_credential();
        }
        self.pending = None;
        self.applied = false;
        self.admin_open = false;
        self.users.clear();
        self.password.clear();
        self.new_password.clear();
        self.reset_password.clear();
        self.reset_user = None;
        self.error.clear();
        self.next_check = 0.;
    }
    pub fn ready(&self) -> bool {
        !self.enabled || self.session.as_ref().is_some_and(|s| s.expires > now() && s.user.active && !s.user.must_change_password)
    }
    fn start(&mut self, ctx: &egui::Context, f: impl FnOnce(String, String) -> Result<Reply, String> + Send + 'static) {
        if self.pending.is_some() {
            return;
        }
        self.error.clear();
        let server = self.server.clone();
        let token = self.session.as_ref().map(|s| s.token.clone()).unwrap_or_default();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(f(server, token));
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) {
        if let Some(rx) = &self.pending {
            if let Ok(result) = rx.try_recv() {
                self.pending = None;
                match result {
                    Ok(Reply::Login(s)) => {
                        self.session = Some(s);
                        self.applied = false;
                        self.password.clear();
                        self.new_password.clear();
                        self.save();
                    }
                    Ok(Reply::Check(u)) => {
                        if let Some(s) = &mut self.session {
                            s.user = u;
                        }
                        self.status.clear();
                        self.save();
                    }
                    Ok(Reply::Users(users, grants)) => {
                        self.users = users;
                        self.grants = grants;
                    }
                    Ok(Reply::Done) => {
                        self.status = "Saved. Account changes take effect on the server immediately.".into();
                        self.reset_user = None;
                        self.reset_password.clear();
                        self.new_password.clear();
                    }
                    Err(e) => {
                        if e.contains("expired or was revoked") || e.contains("Sign in to continue") {
                            self.logout();
                        }
                        self.error = e;
                    }
                }
            }
        }
    }
    fn user_list(&mut self, ctx: &egui::Context) {
        self.start(ctx, |s, t| {
            let users = serde_json::from_value(request(&s, &t, "GET", "/v1/admin/users", None)?).map_err(|_| "Invalid users response")?;
            let grants = serde_json::from_value(request(&s, &t, "GET", "/v1/admin/grants", None)?).map_err(|_| "Invalid grants response")?;
            Ok(Reply::Users(users, grants))
        });
    }
}
impl App {
    /// Returns true while the login screen owns the frame.
    pub(crate) fn account_gate(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.accounts.enabled {
            return false;
        }
        let was_ready = self.accounts.ready();
        self.accounts.poll();
        if (was_ready || self.accounts.applied) && !self.accounts.ready() {
            self.flush_autosave();
            if let Some(lib) = &self.lib {
                if let Ok(text) = self.doc.to_json() {
                    if let Err(e) = std::fs::write(lib.root().join(format!("recovery-{}.a11y", uuid::Uuid::new_v4())), text) {
                        self.accounts.error = format!("Session ended; recovery save failed: {e}. Keep the app open.");
                    }
                }
            }
            self.accounts.applied = false;
            self.end_collab();
            self.stop_bridge();
        }
        if self.accounts.ready() {
            if !self.accounts.applied {
                let s = self.accounts.session.as_ref().unwrap().clone();
                // Cache and local library are partitioned by immutable server/account identity.
                use sha2::{Digest, Sha256};
                let partition: String =
                    Sha256::digest(format!("{}:{}", self.accounts.server, s.user.id).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
                // Preserve the outgoing document before resetting account-scoped editor state.
                // Never silently discard an unsynced draft, including after session expiry.
                if let Some(lib) = &self.lib {
                    let saved = self.doc.to_json().map_err(|e| e.to_string()).and_then(|text| {
                        let tmp = lib.root().join(".account-switch-recovery.tmp");
                        std::fs::write(&tmp, text)
                            .and_then(|_| std::fs::rename(tmp, lib.root().join(format!("recovery-{}.a11y", uuid::Uuid::new_v4()))))
                            .map_err(|e| e.to_string())
                    });
                    if let Err(error) = saved {
                        ui.colored_label(
                            theme::DANGER,
                            format!("Could not preserve your previous document: {error}. Keep A11y open and free disk space before continuing."),
                        );
                        return true;
                    }
                }
                let accounts = std::mem::take(&mut self.accounts);
                self.end_collab();
                self.stop_bridge();
                let updater = std::mem::take(&mut self.updater);
                *self = App::new(crate::demo::demo_document());
                self.accounts = accounts;
                self.updater = updater;
                self.organizations = None;
                self.attach_library(a11y_library::Library::default_root().join("accounts").join(partition));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(lib) = &self.lib {
                        let _ = std::fs::set_permissions(lib.root(), std::fs::Permissions::from_mode(0o700));
                    }
                }
                self.workspace.use_session(&self.accounts.server, &s.token);
                self.set_author(&s.user.username);
                self.go_home();
                if let Some(h) = &mut self.home {
                    h.section = crate::home::Section::Shared;
                }
                self.accounts.applied = true;
            }
            let time = ui.ctx().input(|i| i.time);
            if time >= self.accounts.next_check && self.accounts.pending.is_none() {
                self.accounts.next_check = time + 30.;
                self.accounts.start(ui.ctx(), |s, t| {
                    serde_json::from_value(request(&s, &t, "GET", "/v1/auth/me", None)?).map(Reply::Check).map_err(|_| "Invalid account response".into())
                });
            }
            return false;
        }
        self.stop_bridge();
        if cfg!(windows) {
            ui.horizontal(|ui| {
                let drag = ui.allocate_response(egui::vec2((ui.available_width() - 100.).max(0.), 28.), egui::Sense::drag());
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if ui.button("—").on_hover_text("Minimize").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                if ui.button("□").on_hover_text("Maximize / restore").clicked() {
                    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if ui.button("×").on_hover_text("Close").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
        let mut check_update = false;
        let a = &mut self.accounts;
        ui.vertical_centered(|ui| {
            ui.add_space(64.);
            ui.heading("A11y");
            ui.add_space(8.);
            let changing = a.session.as_ref().is_some_and(|s| s.user.must_change_password);
            ui.label(if changing { "Choose your own password" } else { "Sign in to your workspace" });
            ui.add_space(24.);
            ui.scope(|ui| {
                ui.set_max_width(360.);
                ui.spacing_mut().item_spacing.y = 8.;
                ui.add_enabled_ui(a.pending.is_none(), |ui| {
                    if !changing {
                        ui.label("Server address");
                        ui.text_edit_singleline(&mut a.server);
                        ui.label("Username");
                        ui.text_edit_singleline(&mut a.username);
                    }
                    ui.label(if changing { "Temporary / current password" } else { "Password" });
                    ui.add(egui::TextEdit::singleline(&mut a.password).password(true).desired_width(360.));
                    if changing {
                        ui.label("New password (at least 14 characters)");
                        ui.add(egui::TextEdit::singleline(&mut a.new_password).password(true).desired_width(360.));
                    }
                    ui.checkbox(&mut a.remember, "Remember me on this computer");
                    if ui.button(if changing { "Change password and sign in" } else { "Sign in" }).clicked() {
                        let name = a.username.clone();
                        let pass = std::mem::take(&mut a.password);
                        let new = std::mem::take(&mut a.new_password);
                        a.start(ui.ctx(), move |s, t| {
                            let (path, body) = if changing {
                                ("/v1/auth/password", json!({"current_password":pass,"password":new}))
                            } else {
                                ("/v1/auth/login", json!({"username":name,"password":pass}))
                            };
                            serde_json::from_value(request(&s, &t, "POST", path, Some(body))?).map(Reply::Login).map_err(|_| "Invalid login response".into())
                        });
                    }
                    if changing && ui.button("Use another account").clicked() {
                        a.logout();
                    }
                });
                if a.pending.is_some() {
                    ui.spinner();
                }
                if !a.error.is_empty() {
                    ui.colored_label(theme::DANGER, &a.error);
                }
                ui.add_space(16.);
                ui.label("Your administrator creates your account. Ask them for your username and temporary password.");
                check_update = ui.button("Check for updates").clicked();
            });
        });
        if check_update {
            self.check_for_updates(ui.ctx(), true);
        }
        true
    }
    pub(crate) fn account_admin(&mut self, ctx: &egui::Context) {
        if !self.accounts.admin_open {
            return;
        }
        let a = &mut self.accounts;
        if !a.session.as_ref().is_some_and(|s| s.user.admin && !s.user.must_change_password) {
            a.admin_open = false;
            return;
        }
        let mut open = true;
        egui::Window::new("Admin · People and access").open(&mut open).default_width(680.).resizable(true).show(ctx,|ui|{
            ui.label("Manage accounts for this workspace. Changes apply immediately.");
            if ui.add_enabled(a.pending.is_none(),egui::Button::new("Refresh users")).clicked(){a.user_list(ctx);}
            if a.pending.is_some(){ui.spinner();}
            if !a.error.is_empty(){ui.colored_label(theme::DANGER,&a.error);}
            if !a.status.is_empty(){ui.label(&a.status);}
            ui.separator();
            egui::ScrollArea::vertical().max_height(260.).show(ui,|ui|{
                for u in a.users.clone(){ui.push_id(&u.id,|ui|{ui.horizontal_wrapped(|ui|{
                    ui.strong(&u.username);ui.label(if u.admin{"Administrator"}else{"Member"});ui.label(&u.access);ui.label(if u.active{"Active"}else{"Disabled"});
                    ui.add_enabled_ui(a.pending.is_none(),|ui|{
                        if ui.button(if u.active{"Disable"}else{"Enable"}).clicked(){let body=json!({"id":u.id,"active":!u.active,"admin":u.admin,"access":u.access});a.start(ctx,move|s,t|request(&s,&t,"POST","/v1/admin/update",Some(body)).map(|_|Reply::Done));}
                        if ui.button("Reset password…").clicked(){a.reset_user=Some(u.id.clone());}
                        if ui.button(if u.admin {"Make member"}else{"Make admin"}).clicked(){
                            let body=json!({"id":u.id,"active":u.active,"admin":!u.admin,"access":u.access});
                            a.start(ctx,move|s,t|request(&s,&t,"POST","/v1/admin/update",Some(body)).map(|_|Reply::Done));
                        }
                        if ui.button("Permissions…").clicked(){a.grant_user=u.id.clone();}
                    });
                });});}
            });
            ui.separator();
            egui::CollapsingHeader::new("Create user").default_open(true).show(ui,|ui|{
                ui.label("Username");ui.text_edit_singleline(&mut a.new_user);ui.label("Temporary password · at least 14 characters");ui.add(egui::TextEdit::singleline(&mut a.new_password).password(true));
                ui.checkbox(&mut a.new_admin,"Administrator — can manage all users and workspace files");
                role_picker(ui,"Default library access",&mut a.new_access);
                if ui.add_enabled(a.pending.is_none(),egui::Button::new("Create account")).clicked(){let body=json!({"username":a.new_user,"password":std::mem::take(&mut a.new_password),"admin":a.new_admin,"access":a.new_access});a.start(ctx,move|s,t|request(&s,&t,"POST","/v1/admin/users",Some(body)).map(|_|Reply::Done));}
            });
            if let Some(id)=a.reset_user.clone(){ui.group(|ui|{ui.label("New temporary password");ui.add(egui::TextEdit::singleline(&mut a.reset_password).password(true));if ui.add_enabled(a.pending.is_none(),egui::Button::new("Reset and revoke sessions")).clicked(){let body=json!({"id":id,"password":std::mem::take(&mut a.reset_password)});a.start(ctx,move|s,t|request(&s,&t,"POST","/v1/admin/reset",Some(body)).map(|_|Reply::Done));}});}
            if !a.grant_user.is_empty(){ui.group(|ui|{
                let name=a.users.iter().find(|u|u.id==a.grant_user).map(|u|u.username.as_str()).unwrap_or("Selected user");ui.strong(format!("Permissions · {name}"));
                for g in a.grants.iter().filter(|g|g.user_id==a.grant_user){ui.label(format!("{} {}: {}",g.kind,g.resource,g.role));}
                egui::ComboBox::from_id_salt("grant-kind").selected_text(&a.grant_kind).show_ui(ui,|ui|{ui.selectable_value(&mut a.grant_kind,"project".into(),"Project");ui.selectable_value(&mut a.grant_kind,"file".into(),"File");});
                let files=self.workspace.catalog.files.clone();
                egui::ComboBox::from_id_salt("grant-resource").selected_text(if a.grant_resource.is_empty(){"Choose a resource"}else{&a.grant_resource}).show_ui(ui,|ui|{
                    let mut projects=std::collections::BTreeSet::new();
                    for f in files {
                        if a.grant_kind=="file" {ui.selectable_value(&mut a.grant_resource,f.id,format!("{} / {}",f.project,f.name));}
                        else if projects.insert(f.project.clone()){ui.selectable_value(&mut a.grant_resource,f.project.clone(),f.project);}
                    }
                });role_picker(ui,"Access",&mut a.grant_role);
                ui.small("File rules override project rules; project rules override default access. None explicitly blocks access. Administrators have full access.");
                if ui.add_enabled(a.pending.is_none(),egui::Button::new("Save permission")).clicked(){let body=json!({"user_id":a.grant_user,"kind":a.grant_kind,"resource":a.grant_resource,"role":a.grant_role});a.start(ctx,move|s,t|request(&s,&t,"POST","/v1/admin/grants",Some(body)).map(|_|Reply::Done));}
            });}
        });
        a.admin_open = open;
    }
}
fn role_picker(ui: &mut egui::Ui, label: &str, value: &mut String) {
    egui::ComboBox::from_id_salt(label).selected_text(format!("{label}: {value}")).show_ui(ui, |ui| {
        for r in ["none", "viewer", "editor"] {
            ui.selectable_value(value, r.into(), r);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_gate_requires_active_full_session_and_renders() {
        let mut a = Accounts { enabled: true, ..Default::default() };
        assert!(!a.ready());
        let user = User { id: "test".into(), username: "test".into(), admin: false, active: true, must_change_password: true, access: "viewer".into() };
        a.session = Some(Session { token: "test-token".into(), user, expires: now() + 600 });
        assert!(!a.ready());
        a.session.as_mut().unwrap().user.must_change_password = false;
        assert!(a.ready());
        a.session.as_mut().unwrap().expires = now() - 1;
        assert!(!a.ready());
        let mut app = App::new(crate::demo::demo_document());
        app.accounts.enabled = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| assert!(app.account_gate(ui)));
        assert!(!out.shapes.is_empty());
        assert!(app.accounts.session.is_none());
    }
    #[test]
    fn refuses_plaintext_or_credential_bearing_server_addresses() {
        for server in ["http://127.0.0.1:49180", "https://name:password@example.com", "https://example.com/path", "https://example.com?token=x"] {
            assert!(request(server, "", "POST", "/v1/auth/login", Some(json!({}))).is_err());
        }
    }
}
