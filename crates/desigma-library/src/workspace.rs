//! Persistent catalog shared by the A11y desktop client and library server.
use crate::{new_id, write_atomic};
use desigma_doc::Document;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_UPLOAD: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedFile {
    pub id: String,
    pub name: String,
    pub project: String,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub files: Vec<SharedFile>,
}
#[derive(Serialize, Deserialize)]
pub struct PublishFile {
    pub project: String,
    pub document: String,
}
#[derive(Serialize, Deserialize)]
pub struct OpenFile {
    pub link: String,
}

pub struct WorkspaceStore {
    root: PathBuf,
    pub catalog: Catalog,
}
impl WorkspaceStore {
    pub fn open(root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(root.join("files")).map_err(|e| e.to_string())?;
        let path = root.join("catalog.json");
        let catalog: Catalog = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|e| format!("Invalid workspace catalog: {e}"))?
        } else {
            Catalog::default()
        };
        let mut ids = std::collections::HashSet::new();
        for file in &catalog.files {
            if !valid_id(&file.id) || !ids.insert(&file.id) {
                return Err("Invalid or duplicate file ID in catalog".into());
            }
        }
        Ok(Self { root: root.to_path_buf(), catalog })
    }
    pub fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) || !self.catalog.files.iter().any(|f| f.id == id) {
            return Err("Shared file not found".into());
        }
        Ok(self.root.join("files").join(format!("{id}.desigma")))
    }
    pub fn publish(&mut self, input: PublishFile) -> Result<SharedFile, String> {
        if input.document.len() > MAX_UPLOAD {
            return Err("File exceeds the 32 MB demo limit".into());
        }
        let doc = Document::from_json(&input.document).map_err(|e| e.to_string())?;
        let name = doc.name.trim();
        if name.len() > 256 || input.project.trim().len() > 128 {
            return Err("File or project name is too long".into());
        }
        let file = SharedFile {
            id: new_id(),
            name: if name.is_empty() { "Untitled".into() } else { name.into() },
            project: if input.project.trim().is_empty() { "Shared project".into() } else { input.project.trim().into() },
        };
        let path = self.root.join("files").join(format!("{}.desigma", file.id));
        write_atomic(&path, input.document.as_bytes()).map_err(|e| e.to_string())?;
        let mut next = self.catalog.clone();
        next.files.push(file.clone());
        if let Err(e) = write_atomic(&self.root.join("catalog.json"), &serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?) {
            let _ = std::fs::remove_file(path);
            return Err(e.to_string());
        }
        self.catalog = next;
        Ok(file)
    }
    pub fn listing(&self) -> Catalog {
        let mut result = self.catalog.clone();
        for entry in &mut result.files {
            if let Ok(path) = self.path(&entry.id) {
                if let Ok(text) = std::fs::read_to_string(path) {
                    if let Ok(doc) = Document::from_json(&text) {
                        entry.name = doc.name;
                    }
                }
            }
        }
        result
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_survives_reopen_without_overwriting_original() {
        let root = std::env::temp_dir().join(format!("a11y-catalog-{}", new_id()));
        let mut store = WorkspaceStore::open(&root).unwrap();
        let mut doc = Document::new("Team design");
        doc.add_page("Page 1");
        let entry = store.publish(PublishFile { project: "Design".into(), document: doc.to_json().unwrap() }).unwrap();
        let store = WorkspaceStore::open(&root).unwrap();
        assert_eq!(store.catalog.files[0].id, entry.id);
        assert_eq!(store.listing().files[0].name, "Team design");
        assert!(store.path("../../private").is_err());
        assert!(store.path("deadbeef").is_err());
        let mut store = store;
        assert!(store.publish(PublishFile { project: "x".into(), document: "broken".into() }).is_err());
        assert_eq!(store.catalog.files.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
