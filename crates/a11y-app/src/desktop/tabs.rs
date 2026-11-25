//! Open library documents in the current workspace and their in-window view state.

use crate::{
    app::{App, View},
    theme,
};
use a11y_doc::{Document, NodeId};
use egui::{Rect, Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType};

pub(crate) const HEIGHT: f32 = 38.0;
const HOME_WIDTH: f32 = 42.0;
const MIN_TAB_WIDTH: f32 = 78.0;
const MAX_TAB_WIDTH: f32 = 116.0;

struct Tab {
    id: String,
    name: String,
    view: Option<TabView>,
    shared: bool,
}

struct TabView {
    page: NodeId,
    selection: Vec<NodeId>,
    viewport: View,
    inspect: bool,
}

#[derive(Default)]
pub(crate) struct Tabs {
    pub(crate) shared: Option<ParkedSharedTab>,
    documents: Vec<Tab>,
    previews: crate::home::thumb::Textures,
}

/// The joined editor continues to receive edits while a library file is on screen.
pub(crate) struct ParkedSharedTab {
    pub(crate) editor: Box<App>,
}

impl Drop for ParkedSharedTab {
    fn drop(&mut self) {
        self.editor.end_collab();
    }
}

impl App {
    pub(crate) fn can_park_shared_tab(&self) -> bool {
        cfg!(feature = "enhanced-desktop") && self.collab.is_guest() && self.lib_file.is_none() && self.desktop_tabs.shared.is_none()
    }

    pub(crate) fn park_shared_tab(&mut self) {
        crate::decks::ui::flush_notes(self);
        self.canvas.reset_interaction();
        let author = self.author();
        let doc = std::mem::replace(&mut self.doc, Document::new(""));
        let mut editor = Box::new(App::new(doc));
        editor.comments.author = Some(author);
        editor.now = self.now;
        self.swap_shared_view(&mut editor);
        self.desktop_tabs.shared = Some(ParkedSharedTab { editor });
    }

    pub(crate) fn restore_shared_editor(&mut self, mut tab: ParkedSharedTab) {
        self.save_close_version();
        let doc = std::mem::replace(&mut tab.editor.doc, Document::new(""));
        self.replace_document(doc, None);
        self.swap_shared_view(&mut tab.editor);
        self.sanitize();
        self.changed();
        self.ui.menu = None;
        self.ui.ctx_menu = None;
        self.toast = tab.editor.toast.take();
    }

    // Move the shared editor state without moving window-wide settings or the clipboard.
    fn swap_shared_view(&mut self, editor: &mut App) {
        use std::mem::swap;
        swap(&mut self.collab, &mut editor.collab);
        swap(&mut self.document_session, &mut editor.document_session);
        swap(&mut self.file_hash, &mut editor.file_hash);
        swap(&mut self.hist, &mut editor.hist);
        swap(&mut self.page, &mut editor.page);
        swap(&mut self.selection, &mut editor.selection);
        swap(&mut self.view, &mut editor.view);
        swap(&mut self.canvas, &mut editor.canvas);
        swap(&mut self.canvas_rect, &mut editor.canvas_rect);
        swap(&mut self.first_fit_done, &mut editor.first_fit_done);
        swap(&mut self.fonts, &mut editor.fonts);
        swap(&mut self.tool, &mut editor.tool);
        swap(&mut self.layers, &mut editor.layers);
        swap(&mut self.board, &mut editor.board);
        swap(&mut self.decks, &mut editor.decks);
        swap(&mut self.motion, &mut editor.motion);
        swap(&mut self.motion_ins, &mut editor.motion_ins);
        swap(&mut self.pasted_components, &mut editor.pasted_components);
        swap(&mut self.ui.dev_mode, &mut editor.ui.dev_mode);
    }

    pub(crate) fn remember_document_tab(&mut self) {
        if !cfg!(feature = "enhanced-desktop") {
            return;
        }
        let Some(file) = &self.lib_file else { return };
        if self.desktop_tabs.documents.iter().all(|tab| tab.id != file.id) {
            self.desktop_tabs.documents.push(Tab { id: file.id.clone(), name: file.name.clone(), view: None, shared: false });
        }
    }

    pub(crate) fn capture_tab_view(&mut self) {
        if self.home.is_some() {
            return;
        }
        let Some(file) = &self.lib_file else { return };
        if let Some(tab) = self.desktop_tabs.documents.iter_mut().find(|tab| tab.id == file.id) {
            tab.name.clone_from(&file.name);
            tab.view = Some(TabView { page: self.page, selection: self.selection.clone(), viewport: self.view, inspect: self.ui.dev_mode });
        }
    }

    pub(crate) fn restore_tab_view(&mut self, id: &str) {
        self.ui.dev_mode = false;
        let Some(state) = self.desktop_tabs.documents.iter().find(|tab| tab.id == id).and_then(|tab| tab.view.as_ref()) else {
            return;
        };
        if self.doc.pages.contains(&state.page) {
            self.page = state.page;
            self.selection = state.selection.iter().copied().filter(|id| self.doc.page_of(*id) == Some(state.page)).collect();
            self.view = state.viewport;
            self.first_fit_done = true;
        }
        self.ui.dev_mode = state.inspect;
    }
}

enum Action {
    Home,
    ResumeGuest,
    CloseGuest,
    Open(String),
    Close(String),
}

pub(crate) fn show(app: &mut App, ui: &mut Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, theme::BG_SECONDARY);
    // Register the background first so tabs and caption buttons take precedence.
    let drag = ui.interact(rect, ui.id().with("window-title-drag"), Sense::click_and_drag());
    if drag.double_clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!ui.input(|i| i.viewport().maximized.unwrap_or(false))));
    } else if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let mut content = rect;
    if cfg!(target_os = "macos") && !ui.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        content.min.x = rect.left() + 80.0;
    }
    if cfg!(windows) {
        content.max.x = rect.right() - 138.0;
        window_controls(app, ui, rect);
    }
    let mut bar = ui.new_child(UiBuilder::new().max_rect(content));
    if app.dialog.is_some() {
        bar.disable();
    }
    bar.set_clip_rect(content);
    let mut action = None;
    let home = app.home.is_some();
    let active = app.lib_file.as_ref().filter(|_| !home);
    let guest_editor = if app.collab.is_guest() { Some(&*app) } else { app.desktop_tabs.shared.as_ref().map(|tab| tab.editor.as_ref()) };
    let guest = guest_editor.map(|editor| Tab {
        id: format!("guest:{}", editor.document_session),
        name: editor.doc.name.clone(),
        view: None,
        shared: true,
    });
    let guest_active = app.collab.is_guest() && !home;
    let active_id = active.map(|file| file.id.as_str()).or_else(|| guest.as_ref().filter(|_| guest_active).map(|tab| tab.id.as_str()));
    let reveal_key = ui.id().with("desktop-last-active-tab");
    let reveal = ui.data_mut(|data| {
        let previous = data.get_temp::<String>(reveal_key);
        let changed = previous.as_deref() != active_id;
        if changed {
            if let Some(id) = active_id {
                data.insert_temp(reveal_key, id.to_owned());
            } else {
                data.remove::<String>(reveal_key);
            }
        }
        changed
    });
    let home_rect = Rect::from_min_size(content.min, Vec2::new(HOME_WIDTH.min(content.width()), content.height()));
    if home_button(&mut bar, home_rect, home) && !home {
        action = Some(Action::Home);
    }

    let strip_rect = Rect::from_min_max(home_rect.right_top(), content.max);
    let count = app.desktop_tabs.documents.len() + guest.iter().len();
    if count > 0 && strip_rect.width() > 0.0 {
        let width = (strip_rect.width() / count as f32).clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
        let mut strip = bar.new_child(UiBuilder::new().max_rect(strip_rect));
        strip.set_clip_rect(strip_rect);
        egui::ScrollArea::horizontal().id_salt("desktop-document-tabs").auto_shrink([false, true]).show(&mut strip, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let Tabs { documents, previews, .. } = &mut app.desktop_tabs;
                for tab in documents {
                    let current = active.filter(|file| file.id == tab.id);
                    let name = current.map_or(tab.name.as_str(), |file| file.name.as_str());
                    let (selected, closed, rect) = document_button(ui, tab, name, current.is_some(), width, app.lib.as_ref(), previews);
                    if current.is_some() && reveal {
                        ui.scroll_to_rect(rect, None);
                    }
                    if closed {
                        action = Some(Action::Close(tab.id.clone()));
                    } else if selected && current.is_none() {
                        action = Some(Action::Open(tab.id.clone()));
                    }
                }
                if let Some(tab) = &guest {
                    let (selected, closed, rect) = document_button(ui, tab, &tab.name, guest_active, width, None, previews);
                    if guest_active && reveal {
                        ui.scroll_to_rect(rect, None);
                    }
                    if closed {
                        action = Some(Action::CloseGuest);
                    } else if selected && !guest_active {
                        action = Some(Action::ResumeGuest);
                    }
                }
            });
        });
    }
    match action {
        Some(Action::Home) => app.open_desktop_home(),
        Some(Action::ResumeGuest) => app.resume_shared_tab(),
        Some(Action::CloseGuest) => app.close_shared_tab(),
        Some(Action::Open(id)) => {
            app.open_library_file(&id);
            if app.home.is_none() && app.lib_file.as_ref().is_some_and(|file| file.id == id) {
                ui.memory_mut(|memory| {
                    if let Some(focused) = memory.focused() {
                        memory.surrender_focus(focused);
                    }
                });
            }
        }
        Some(Action::Close(id)) => close(app, id),
        None => {}
    }
}

fn tab_background(ui: &Ui, rect: Rect, active: bool, hovered: bool) {
    let color = if active {
        theme::BG_MENU
    } else if hovered {
        theme::BG_SECONDARY
    } else {
        theme::BG_TERTIARY
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, color);
    let divider_x = rect.right() - 0.5;
    painter.line_segment([egui::pos2(divider_x, rect.top()), egui::pos2(divider_x, rect.bottom())], Stroke::new(1.0, theme::BORDER_TRANSLUCENT));
}

fn document_button(
    ui: &mut Ui,
    tab: &Tab,
    name: &str,
    active: bool,
    width_limit: f32,
    library: Option<&a11y_library::Library>,
    previews: &mut crate::home::thumb::Textures,
) -> (bool, bool, Rect) {
    let font = theme::font_body();
    // Reserve the close-button width in both states so selection doesn't move other tabs.
    let width = (crate::widgets::text_width(ui.painter(), name, &font) + 40.0).ceil().clamp(MIN_TAB_WIDTH, width_limit);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::hover());
    let show_close = active || (tab.shared && ui.rect_contains_pointer(rect));
    let mut label_rect = rect;
    if show_close {
        label_rect.max.x -= 24.0;
    }
    let response = ui.interact(label_rect, ui.id().with(("document-tab", &tab.id)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, active, name));
    let hovered = response.hovered();
    tab_background(ui, rect, active, hovered);

    let color = if active || hovered { theme::TEXT } else { theme::TEXT_SECONDARY };
    let text_rect = label_rect.shrink2(Vec2::new(8.0, 0.0));
    let title = crate::widgets::elide(ui.painter(), name, &font, text_rect.width());
    ui.painter().with_clip_rect(text_rect).text(text_rect.left_center(), egui::Align2::LEFT_CENTER, &title, font, color);
    if response.has_focus() {
        ui.painter().rect_stroke(label_rect.shrink(1.0), theme::cr(theme::RADIUS_MEDIUM), Stroke::new(1.0, theme::ICON_SECONDARY), egui::StrokeKind::Inside);
    }
    let open = response.clicked();
    let mut closed = false;
    if show_close {
        let close_rect = Rect::from_min_max(label_rect.right_top(), rect.right_bottom());
        let close = ui.interact(close_rect, ui.id().with(("close-document-tab", &tab.id)), Sense::click());
        close.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Close {name}")));
        if close.hovered() {
            ui.painter().rect_filled(close_rect.shrink(2.0), theme::cr(theme::RADIUS_MEDIUM), theme::BG_TERTIARY);
        }
        ui.painter().text(close_rect.center(), egui::Align2::CENTER_CENTER, "×", theme::font_body(), theme::TEXT);
        closed = close.on_hover_text(format!("Close {name}")).clicked();
        if title != name {
            response.on_hover_text(name);
        }
    } else if !open {
        response.on_hover_ui(|ui| {
            use crate::home::time::{edited, wall_ms};
            ui.set_width(240.0);
            let entry = library.and_then(|lib| lib.file(&tab.id).ok());
            let thumbnail = library.zip(entry.as_ref()).and_then(|(lib, entry)| {
                let mut budget = 1;
                previews.get(ui.ctx(), lib, entry, &mut budget)
            });
            let (area, _) = ui.allocate_exact_size(Vec2::new(240.0, 135.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(area, 4.0, theme::BG_SECONDARY);
            if let Some((texture, size)) = thumbnail.filter(|(_, size)| size.x > 0.0 && size.y > 0.0) {
                let scale = (area.width() / size.x).min(area.height() / size.y);
                let image = Rect::from_center_size(area.center(), size * scale);
                painter.image(texture, image, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            } else {
                painter.text(area.center(), egui::Align2::CENTER_CENTER, "No preview available", theme::font_body(), theme::TEXT_SECONDARY);
            }
            ui.label(name);
            if let Some(entry) = entry {
                ui.label(edited(wall_ms(), entry.modified));
            }
        });
    }
    (open, closed, rect)
}

fn home_button(ui: &mut Ui, rect: Rect, active: bool) -> bool {
    let response = ui.interact(rect, ui.id().with("desktop-home-tab"), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, active, "Home"));
    let painter = ui.painter();
    tab_background(ui, rect, active, response.hovered());
    let center = rect.center();
    let stroke = Stroke::new(1.4, if active || response.hovered() { theme::TEXT } else { theme::ICON_SECONDARY });
    let outline = [(-7.0, -1.0), (0.0, -7.0), (7.0, -1.0), (7.0, 7.0), (2.5, 7.0), (2.5, 2.0), (-2.5, 2.0), (-2.5, 7.0), (-7.0, 7.0), (-7.0, -1.0)];
    painter.add(egui::Shape::line(outline.into_iter().map(|(x, y)| center + Vec2::new(x, y)).collect(), stroke));
    response.on_hover_text("Home").clicked()
}

fn close(app: &mut App, id: String) {
    let active = app.home.is_none() && app.lib_file.as_ref().is_some_and(|file| file.id == id);
    if active {
        let again = id.clone();
        if !app.leave_ok(move |app| close(app, again)) {
            return;
        }
        app.go_home();
        if app.home.is_none() {
            return;
        }
    }
    app.desktop_tabs.documents.retain(|tab| tab.id != id);
}

/// Windows caption buttons share the same row as the document tabs.
fn window_controls(app: &mut App, ui: &mut Ui, rect: Rect) {
    use egui::{Align2, Pos2, ViewportCommand};
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    for (index, label, glyph, command) in [
        (0, "Minimize", "−", ViewportCommand::Minimized(true)),
        (1, if maximized { "Restore" } else { "Maximize" }, if maximized { "❐" } else { "□" }, ViewportCommand::Maximized(!maximized)),
        (2, "Close window", "×", ViewportCommand::Close),
    ] {
        let button = Rect::from_min_size(Pos2::new(rect.right() - 138.0 + index as f32 * 46.0, rect.top()), Vec2::new(46.0, rect.height()));
        let response = ui.interact(button, ui.id().with(("caption", index)), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        if response.hovered() {
            ui.painter().rect_filled(button, 0.0, if index == 2 { egui::Color32::from_rgb(196, 43, 28) } else { theme::BG_TERTIARY });
        }
        ui.painter().text(button.center(), Align2::CENTER_CENTER, glyph, theme::font_large_strong(), theme::TEXT);
        if response.on_hover_text(label).clicked() {
            if index == 2 {
                app.request_quit(ui.ctx());
            } else {
                ui.ctx().send_viewport_cmd(command);
            }
        }
    }
}

/// Restore edge resizing when Windows' native title bar is replaced.
pub(crate) fn resize_edges(ctx: &egui::Context, rect: Rect) {
    use egui::{CursorIcon, ResizeDirection as D, ViewportCommand};
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let Some(p) = ctx.pointer_hover_pos().filter(|p| rect.contains(*p)) else { return };
    let left = p.x - rect.left() < 6.0;
    let right = rect.right() - p.x < 6.0;
    let top = p.y - rect.top() < 6.0;
    let bottom = rect.bottom() - p.y < 6.0;
    let edge = match (left, right, top, bottom) {
        (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNwSe)),
        (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNeSw)),
        (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeNeSw)),
        (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeNwSe)),
        (true, _, _, _) => Some((D::West, CursorIcon::ResizeHorizontal)),
        (_, true, _, _) => Some((D::East, CursorIcon::ResizeHorizontal)),
        (_, _, true, _) => Some((D::North, CursorIcon::ResizeVertical)),
        (_, _, _, true) => Some((D::South, CursorIcon::ResizeVertical)),
        _ => None,
    };
    if let Some((direction, cursor)) = edge {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}
