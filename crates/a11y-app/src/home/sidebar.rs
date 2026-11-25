//! Left sidebar of the home screen: search, Recents / Starred, the project
//! tree with its folders and files, and the Trash.

use super::{Act, DropTo, Home, Section, Target};
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets;
use a11y_library::{FileEntry, Project};
use egui::{Align2, Id, Pos2, Rect, Response, Sense, Ui, Vec2};
use std::collections::HashMap;

const ROW_H: f32 = 30.0;
const INDENT: f32 = 16.0;

pub fn show(
    h: &mut Home,
    ui: &mut Ui,
    rect: Rect,
    acts: &mut Vec<Act>,
    workspace: Option<&a11y_library::organizations::Workspace>,
    profile: Option<&str>,
    admin: bool,
) {
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme::BG);
    widgets::vline(&p, rect.right() - theme::BORDER_WIDTH, rect.top(), rect.bottom(), theme::BORDER);
    let x0 = rect.left();
    let w = rect.width() - theme::BORDER_WIDTH;
    let mut y = rect.top();

    if let Some(name) = profile {
        let profile_rect = Rect::from_min_size(Pos2::new(x0 + 12.0, y + 8.0), Vec2::new(w - 24.0, 40.0));
        crate::desktop::profile::show(ui, profile_rect, name, &mut h.profile_menu, acts, admin);
    } else {
        icons::paint(&p, Icon::Logo, Pos2::new(x0 + 28.0, y + 28.0), 24.0, theme::ICON);
        p.text(Pos2::new(x0 + 48.0, y + 28.0), Align2::LEFT_CENTER, crate::theme::display_title(), theme::font_large_strong(), theme::TEXT);
    }
    y += 56.0;

    if let Some(workspace) = workspace {
        let r = Rect::from_min_size(Pos2::new(x0 + 12.0, y), Vec2::new(w - 24.0, ROW_H));
        crate::organizations::ui::switcher(workspace, ui, r, &mut h.organization_menu, acts);
        y += ROW_H + 8.0;
    }

    // ---- search
    let sr = Rect::from_min_size(Pos2::new(x0 + 12.0, y), Vec2::new(w - 24.0, 32.0));
    p.rect_filled(sr, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SECONDARY);
    icons::paint(&p, Icon::Search, Pos2::new(sr.left() + 16.0, sr.center().y), 16.0, theme::ICON_SECONDARY);
    let tr = Rect::from_min_max(Pos2::new(sr.left() + 32.0, sr.top()), sr.max - Vec2::new(8.0, 0.0));
    let tid = Id::new("home-search");
    let mut text = h.search.clone();
    let te = egui::TextEdit::singleline(&mut text)
        .id(tid)
        .font(theme::font_body())
        .text_color(theme::TEXT)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .vertical_align(egui::Align::Center)
        .hint_text(egui::RichText::new(crate::i18n::tr("Search files")).font(theme::font_body()).color(theme::TEXT_TERTIARY))
        .min_size(tr.size())
        .desired_width(tr.width());
    ui.put(tr, te);
    widgets::record(ui, "home-search", sr);
    if text != h.search {
        h.search = text;
        h.selected = None;
        h.scroll = 0.0;
    }
    y += 32.0 + 12.0;

    // ---- Recents / Starred
    for (icon, label, section, key) in [
        (Icon::Folder, "Shared library", Section::Shared, "shared"),
        (Icon::Clock, "Recents", Section::Recents, "recents"),
        (Icon::Star, "Starred", Section::Starred, "starred"),
    ] {
        let r = Rect::from_min_size(Pos2::new(x0 + 8.0, y), Vec2::new(w - 16.0, ROW_H));
        let selected = h.section == section && (!h.searching() || h.section == Section::Shared);
        if row(ui, r, Id::new(("home-nav", key)), icon, crate::i18n::tr(label), 0.0, selected, false, key).clicked() {
            acts.push(Act::Go(section));
        }
        y += ROW_H;
    }
    y += 12.0;

    // ---- Projects header
    p.text(Pos2::new(x0 + 20.0, y + 14.0), Align2::LEFT_CENTER, crate::i18n::tr("Projects"), theme::font_body_strong(), theme::TEXT_SECONDARY);
    if workspace.is_some() {
        let r = Rect::from_min_size(Pos2::new(x0 + 8.0, y), Vec2::new(w - 48.0, ROW_H));
        if ui.interact(r, Id::new("home-projects"), Sense::click()).clicked() {
            acts.push(Act::Go(Section::Projects));
        }
    }
    let plus = Rect::from_min_size(Pos2::new(x0 + w - 12.0 - 24.0, y + 2.0), Vec2::splat(24.0));
    if widgets::icon_button(ui, plus, "home-new-project", Icon::Plus, false, "New project").clicked() {
        acts.push(Act::NewProject);
    }
    widgets::record(ui, "home-new-project", plus);
    y += 28.0;

    // ---- project tree (scrolls)
    let trash_h = ROW_H + 16.0;
    let list = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x0 + w, (rect.bottom() - trash_h).max(y)));
    let rows = tree_rows(h);
    let content_h = rows.len() as f32 * ROW_H + 8.0;
    crate::panels::wheel_scroll(ui, list, &mut h.side_scroll, content_h);
    let scroll = h.side_scroll;
    let ptr = ui.input(|i| i.pointer.hover_pos());
    let dragging = h.drag.is_some();
    let mut to_toggle = None;
    let mut secondary: Option<(Target, Pos2)> = None;
    let mut dropzones: Vec<(Rect, DropTo)> = vec![];
    crate::panels::clipped(ui, list, |ui| {
        if rows.is_empty() {
            ui.painter().text(
                Pos2::new(x0 + 20.0, list.top() + 16.0),
                Align2::LEFT_CENTER,
                crate::i18n::tr("No projects yet"),
                theme::font_body(),
                theme::TEXT_TERTIARY,
            );
        }
        for (i, tr) in rows.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(x0 + 8.0, list.top() + i as f32 * ROW_H - scroll), Vec2::new(w - 16.0, ROW_H));
            if r.bottom() < list.top() || r.top() > list.bottom() {
                continue;
            }
            let (icon, selected, drop_to) = match &tr.target {
                Target::Project(project) => {
                    (Icon::Project, h.section == Section::Project(project.clone(), None), Some(DropTo::Project(project.clone())))
                }
                Target::Folder(project, folder) => (
                    Icon::Folder,
                    h.section == Section::Project(project.clone(), Some(folder.clone())),
                    Some(DropTo::Folder(project.clone(), folder.clone())),
                ),
                Target::File(id) => (Icon::File, h.selected.as_ref() == Some(id), None),
            };
            let drop_hover = drop_to.is_some() && dragging && ptr.is_some_and(|p| r.contains(p) && list.contains(p));
            let record = if matches!(tr.target, Target::File(_)) { "tree-file" } else { "project" };
            let resp = row(
                ui,
                r,
                Id::new(("home-tree", &tr.key)),
                icon,
                &tr.label,
                tr.depth as f32 * INDENT,
                selected && !h.searching(),
                drop_hover,
                &format!("{record}-{}", tr.label),
            );
            if let Some(open) = tr.expanded {
                let c = Pos2::new(r.left() + 10.0 + tr.depth as f32 * INDENT, r.center().y);
                icons::paint(ui.painter(), if open { Icon::ChevronDown } else { Icon::ChevronRight }, c, 12.0, theme::ICON_TERTIARY);
            }
            let on_chevron = tr.expanded.is_some() && ptr.is_some_and(|p| p.x < r.left() + 22.0 + tr.depth as f32 * INDENT);
            if resp.clicked() {
                if on_chevron {
                    to_toggle = Some(tr.key.clone());
                } else {
                    acts.push(match &tr.target {
                        Target::Project(project) => Act::Go(Section::Project(project.clone(), None)),
                        Target::Folder(project, folder) => Act::Go(Section::Project(project.clone(), Some(folder.clone()))),
                        Target::File(id) => Act::Open(id.clone()),
                    });
                }
            }
            if resp.secondary_clicked() {
                let at = ui.input(|i| i.pointer.interact_pos()).unwrap_or(r.left_bottom());
                secondary = Some((tr.target.clone(), at));
            }
            if let Some(to) = drop_to {
                dropzones.push((r.intersect(list), to));
            }
        }
    });
    h.drop_rects.extend(dropzones);
    crate::panels::scrollbar(ui, list, scroll, content_h);
    if let Some(key) = to_toggle {
        if !h.collapsed.remove(&key) {
            h.collapsed.insert(key);
        }
    }
    if secondary.is_some() {
        h.menu = secondary;
    }

    // ---- Trash
    widgets::hline(&p, x0, x0 + w, rect.bottom() - trash_h, theme::BORDER);
    let tr = Rect::from_min_size(Pos2::new(x0 + 8.0, rect.bottom() - trash_h + 8.0), Vec2::new(w - 16.0, ROW_H));
    let n = h.snap.trash.len();
    let label = if n > 0 { format!("Trash ({n})") } else { "Trash".to_string() };
    if row(ui, tr, Id::new("home-trash"), Icon::Trash, &crate::i18n::tr_text(&label), 0.0, h.section == Section::Trash && !h.searching(), false, "trash")
        .clicked()
    {
        acts.push(Act::Go(Section::Trash));
    }
}

struct TreeRow {
    key: String,
    label: String,
    depth: usize,
    /// `Some(open)` when the row can be expanded.
    expanded: Option<bool>,
    target: Target,
}

type FolderFiles<'a> = HashMap<(&'a str, &'a str), Vec<&'a FileEntry>>;

/// Projects, folders and files within folders, flattened for rendering.
fn tree_rows(h: &Home) -> Vec<TreeRow> {
    fn walk(out: &mut Vec<TreeRow>, h: &Home, files: &FolderFiles<'_>, project: &Project, parent: Option<&str>, depth: usize) {
        let mut folders: Vec<_> = project.folders.iter().filter(|folder| folder.parent.as_deref() == parent).collect();
        folders.sort_by_key(|folder| folder.name.to_lowercase());
        for folder in folders {
            let key = format!("{}/{}", project.id, folder.id);
            let open = !h.collapsed.contains(&key);
            let children = files.get(&(project.id.as_str(), folder.id.as_str()));
            let has_subfolders = project.folders.iter().any(|child| child.parent.as_deref() == Some(folder.id.as_str()));
            out.push(TreeRow {
                key,
                label: folder.name.clone(),
                depth,
                expanded: (children.is_some() || has_subfolders).then_some(open),
                target: Target::Folder(project.id.clone(), folder.id.clone()),
            });
            if open {
                walk(out, h, files, project, Some(&folder.id), depth + 1);
                if let Some(children) = children {
                    out.extend(children.iter().map(|file| TreeRow {
                        key: format!("file/{}", file.id),
                        label: file.name.clone(),
                        depth: depth + 1,
                        expanded: None,
                        target: Target::File(file.id.clone()),
                    }));
                }
            }
        }
    }

    let mut files = FolderFiles::new();
    for file in &h.snap.files {
        if let Some(folder) = file.folder.as_deref() {
            files.entry((file.project.as_str(), folder)).or_default().push(file);
        }
    }
    for children in files.values_mut() {
        children.sort_by_cached_key(|file| file.name.to_lowercase());
    }

    let mut out = vec![];
    for project in &h.snap.projects {
        let open = !h.collapsed.contains(&project.id);
        out.push(TreeRow {
            key: project.id.clone(),
            label: project.name.clone(),
            depth: 0,
            expanded: (!project.folders.is_empty()).then_some(open),
            target: Target::Project(project.id.clone()),
        });
        if open {
            walk(&mut out, h, &files, project, None, 1);
        }
    }
    out
}

/// One sidebar row: icon, label, hover / selected / drop-target highlight.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, r: Rect, id: Id, icon: Icon, label: &str, indent: f32, selected: bool, drop_hover: bool, record: &str) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    if drop_hover {
        p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SELECTED_HOVER);
    } else if selected {
        p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SELECTED);
    } else if resp.hovered() {
        p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_HOVER);
    }
    let ix = r.left() + 12.0 + indent;
    icons::paint(p, icon, Pos2::new(ix + 14.0, r.center().y), 16.0, if selected { theme::ICON } else { theme::ICON_SECONDARY });
    let font = if selected { theme::font_body_strong() } else { theme::font_body() };
    let shown = widgets::elide(p, label, &font, r.right() - (ix + 30.0) - 8.0);
    p.text(Pos2::new(ix + 30.0, r.center().y), Align2::LEFT_CENTER, shown, font, theme::TEXT);
    widgets::record(ui, &format!("home-{record}"), r);
    resp
}
