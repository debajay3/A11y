//! Organization selection for the native home screen.

pub(crate) mod logo;
pub(crate) mod projects;
pub(crate) mod ui;

use crate::{
    app::App,
    home::{Home, Section},
};
use a11y_library::{organizations::Workspace, Library};
use std::path::PathBuf;

impl App {
    pub(crate) fn workspace_root(&mut self, local_root: PathBuf) -> PathBuf {
        match Workspace::open(local_root.clone()) {
            Ok(workspace) => {
                let root = workspace.active_root();
                self.organizations = Some(workspace);
                root
            }
            Err(error) => {
                self.show_toast(format!("Cannot open organizations: {error}"), true);
                local_root
            }
        }
    }

    pub(crate) fn switch_organization(&mut self, id: Option<String>) {
        if self.home.is_none() {
            self.go_home();
            if self.home.is_none() {
                return;
            }
        }
        if let Some(workspace) = self.organizations.as_mut() {
            let result = workspace.select(id);
            self.apply_workspace(result);
        }
    }

    #[cfg(test)]
    pub(crate) fn create_organization(&mut self, name: &str) -> bool {
        self.create_organization_with_logo(name, None)
    }

    pub(crate) fn create_organization_with_logo(&mut self, name: &str, logo: Option<Vec<u8>>) -> bool {
        if self.home.is_none() {
            self.go_home();
            if self.home.is_none() {
                return false;
            }
        }
        let Some(workspace) = self.organizations.as_mut() else { return false };
        let result = workspace.create_with_logo(name, logo);
        self.apply_workspace(result)
    }

    pub(crate) fn edit_organization(&mut self, id: &str, name: &str, logo: Option<Vec<u8>>) -> bool {
        let Some(workspace) = self.organizations.as_mut() else { return false };
        match workspace.edit(id, name, logo) {
            Ok(()) => {
                self.show_toast("Organization updated", false);
                true
            }
            Err(error) => {
                self.show_toast(error.to_string(), true);
                false
            }
        }
    }

    fn apply_workspace(&mut self, result: a11y_library::Result<Library>) -> bool {
        let library = match result {
            Ok(library) => library,
            Err(error) => {
                self.show_toast(error.to_string(), true);
                return false;
            }
        };
        self.ai_cancel();
        self.stop_plugin();
        self.stop_bridge();
        self.end_collab();
        self.comments.author = None;
        self.lib_file = None;
        self.desktop_tabs = Default::default();
        self.parked_home = None;
        self.autosave = Default::default();
        self.libs.set_root(library.root().to_path_buf());
        let mut home = Home::new();
        home.section = Section::Projects;
        home.refresh(&library);
        self.lib = Some(library);
        self.home = Some(home);
        self.load_ai_settings();
        true
    }
}
