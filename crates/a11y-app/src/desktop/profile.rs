use crate::{theme, widgets};
use egui::{Align2, Id, Rect, Sense, Ui, Vec2};

pub(crate) fn show(ui: &mut Ui, area: Rect, name: &str, anchor: &mut Option<Rect>, actions: &mut Vec<crate::home::Act>, admin: bool) {
    let response = ui.interact(area, Id::new("home-profile"), Sense::click());
    let painter = ui.painter().with_clip_rect(area);
    widgets::button_bg(&painter, area, &response, false, theme::cr(theme::RADIUS_MEDIUM));
    let center = area.left_center() + Vec2::new(21.0, 0.0);
    painter.circle_filled(center, 13.0, theme::BG_SECONDARY);
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    painter.text(center, Align2::CENTER_CENTER, initial, theme::font_body_strong(), theme::TEXT);
    let name_rect = Rect::from_min_max(area.min + Vec2::new(42.0, 0.0), area.max - Vec2::new(32.0, 0.0));
    painter.with_clip_rect(name_rect).text(name_rect.left_center(), Align2::LEFT_CENTER, name, theme::font_body_strong(), theme::TEXT);
    crate::icons::paint(&painter, crate::icons::Icon::ChevronDown, area.right_center() - Vec2::new(16.0, 0.0), 12.0, theme::ICON_SECONDARY);
    widgets::record(ui, "home-profile", area);
    if response.on_hover_text("Profile menu").clicked() {
        *anchor = if anchor.is_some() { None } else { Some(area) };
    }
    use crate::widgets::menu::{popup_menu, MenuItem, MenuResult};
    if let Some(rect) = *anchor {
        let mut items = vec![MenuItem::new("Edit display name"), MenuItem::new("Sign out")];
        if admin {
            items.push(MenuItem::new("Admin · People and access"));
        }
        match popup_menu(ui.ctx(), Id::new("profile-menu"), rect.left_bottom(), rect.width(), &items, Some(rect)) {
            MenuResult::Picked(index) => {
                *anchor = None;
                actions.push(match index {
                    0 => crate::home::Act::EditProfile,
                    2 => crate::home::Act::Admin,
                    _ => crate::home::Act::SignOut,
                });
            }
            MenuResult::Closed => *anchor = None,
            MenuResult::Open => {}
        }
    }
}
