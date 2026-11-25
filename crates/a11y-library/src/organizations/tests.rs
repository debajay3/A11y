use super::*;

#[test]
fn organization_switching_persists_selection_and_separates_projects() {
    let root = std::env::temp_dir().join(format!("a11y-workspace-{}", new_id()));
    let mut workspace = Workspace::open(root.clone()).unwrap();
    Library::open(workspace.active_root()).unwrap().create_project("Local project").unwrap();
    let first = workspace.create("First organization").unwrap();
    first.create_project("First project").unwrap();
    let first_id = workspace.active().unwrap().to_owned();
    let second = workspace.create("Second organization").unwrap();
    assert!(second.list_projects().is_empty());
    assert!(workspace.create("first organization").is_err());
    let mut reopened = Workspace::open(root.clone()).unwrap();
    assert_eq!(reopened.organizations().len(), 2);
    assert_eq!(reopened.active_root(), second.root());
    assert_eq!(reopened.select(Some(first_id)).unwrap().list_projects()[0].name, "First project");
    assert_eq!(reopened.select(None).unwrap().list_projects()[0].name, "Local project");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn organization_details_persist_without_changing_project_identity() {
    let root = std::env::temp_dir().join(format!("a11y-workspace-{}", new_id()));
    let mut workspace = Workspace::open(root.clone()).unwrap();
    let logo = vec![255; LOGO_SIDE * LOGO_SIDE * 4];
    workspace.create_with_logo("First", Some(logo.clone())).unwrap().create_project("Design").unwrap();
    let id = workspace.active().unwrap().to_owned();
    let before = workspace.active_root();
    assert_eq!(Workspace::open(root.clone()).unwrap().organizations()[0].logo.as_deref(), Some(logo.as_slice()));
    assert!(workspace.edit(&id, "Invalid", Some(vec![0])).is_err());
    workspace.edit(&id, "Renamed", None).unwrap();
    let reopened = Workspace::open(root.clone()).unwrap();
    assert_eq!(reopened.active_root(), before);
    assert_eq!(reopened.organizations()[0].name, "Renamed");
    assert!(reopened.organizations()[0].logo.is_none());
    assert_eq!(Library::open(before).unwrap().list_projects()[0].name, "Design");
    fs::remove_dir_all(root).unwrap();
}
