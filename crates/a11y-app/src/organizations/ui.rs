//! The organization selector reuses the native sidebar and menu styling.

use crate::{
    home::Act,
    theme,
    widgets::menu::{popup_menu, MenuBadge, MenuItem, MenuResult},
};
use a11y_library::organizations::Workspace;
use egui::{Align2, Id, Rect, Sense, Ui, Vec2};

pub(crate) fn switcher(workspace: &Workspace, ui: &mut Ui, rect: Rect, anchor: &mut Option<Rect>, actions: &mut Vec<Act>) {
    let active = workspace.active();
    let organizations = workspace.organizations();
    let current = organizations.iter().find(|org| Some(org.id.as_str()) == active);
    let label = current.map_or("Local workspace", |org| org.name.as_str());
    let response = ui.interact(rect, Id::new("workspace-switcher"), Sense::click());
    let painter = ui.painter();
    crate::widgets::button_bg(painter, rect, &response, anchor.is_some(), theme::cr(theme::RADIUS_MEDIUM));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Organization: {label}")));
    let badge = Rect::from_center_size(rect.left_center() + Vec2::new(18.0, 0.0), Vec2::splat(20.0));
    super::logo::paint(ui, badge, label, current.and_then(|org| org.logo.as_deref()));
    painter.with_clip_rect(Rect::from_min_max(rect.min + Vec2::new(34.0, 0.0), rect.max - Vec2::new(34.0, 0.0))).text(
        rect.left_center() + Vec2::new(34.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::font_body_strong(),
        theme::TEXT,
    );
    crate::icons::paint(painter, crate::icons::Icon::ChevronDown, rect.right_center() - Vec2::new(16.0, 0.0), 12.0, theme::ICON_SECONDARY);
    if response.clicked() {
        *anchor = if anchor.is_some() { None } else { Some(rect) };
    }
    if let Some(org) = current {
        if response.secondary_clicked() {
            *anchor = None;
        }
        response.context_menu(|ui| {
            if ui.button(crate::i18n::tr("Edit organization")).clicked() {
                actions.push(Act::EditOrganization(org.id.clone()));
                ui.close();
            }
        });
    }
    let Some(menu_rect) = *anchor else { return };
    let mut entries = vec![MenuItem::new("Local workspace").checked(active.is_none())];
    entries.extend(organizations.iter().map(|org| {
        let mut item = MenuItem::new(org.name.as_str()).checked(Some(org.id.as_str()) == active);
        let initial = org.name.trim().chars().next().into_iter().flat_map(char::to_uppercase).collect();
        let colors = [
            egui::Color32::from_rgb(176, 64, 44),
            egui::Color32::from_rgb(106, 65, 182),
            egui::Color32::from_rgb(38, 104, 166),
            egui::Color32::from_rgb(37, 115, 90),
        ];
        let color_index = org.id.bytes().map(usize::from).sum::<usize>() % colors.len();
        let key = Id::new(("organization-menu-logo", &org.id));
        item.badge = Some(MenuBadge {
            texture: super::logo::texture(ui.ctx(), key, org.logo.as_deref()),
            initial,
            fill: colors[color_index],
        });
        item
    }));
    entries.last_mut().unwrap().separator_after = true;
    entries.push(MenuItem::new("Create organization").icon(crate::icons::Icon::Plus));
    match popup_menu(ui.ctx(), Id::new("workspace-switcher-menu"), menu_rect.left_bottom(), rect.width(), &entries, Some(menu_rect)) {
        MenuResult::Picked(index) => {
            *anchor = None;
            if index == organizations.len() + 1 {
                actions.push(Act::NewOrganization);
            } else {
                let id = index.checked_sub(1).map(|i| organizations[i].id.clone());
                if id.as_deref() != active {
                    actions.push(Act::SwitchOrganization(id));
                }
            }
        }
        MenuResult::Closed => *anchor = None,
        MenuResult::Open => {}
    }
}
