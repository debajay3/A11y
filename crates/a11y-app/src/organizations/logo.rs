//! Logo selection and display for the local organization catalog.

use std::{fs::File, io::{Cursor, Read}, path::Path};

use a11y_library::organizations::LOGO_SIDE;
use egui::{Align2, Color32, Id, Pos2, Rect, TextureHandle, Ui, Vec2};

use crate::{app::App, theme};

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

fn load(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err("Choose an image smaller than 10 MB".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let side = LOGO_SIDE as u32;
    let scaled = reader.decode()?.resize(side, side, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut canvas = image::RgbaImage::new(side, side);
    image::imageops::replace(&mut canvas, &scaled, i64::from((side - scaled.width()) / 2), i64::from((side - scaled.height()) / 2));
    Ok(canvas.into_raw())
}

pub(super) fn texture(ctx: &egui::Context, key: Id, logo: Option<&[u8]>) -> Option<TextureHandle> {
    let pixels = logo.filter(|pixels| pixels.len() == LOGO_SIDE * LOGO_SIDE * 4)?;
    let hash = Id::new(pixels);
    let cached = ctx.data(|data| data.get_temp::<(Id, TextureHandle)>(key));
    if let Some((_, texture)) = cached.filter(|(id, _)| *id == hash) {
        return Some(texture);
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([LOGO_SIDE, LOGO_SIDE], pixels);
    let texture = ctx.load_texture("organization-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(key, (hash, texture.clone())));
    Some(texture)
}

pub(super) fn paint(ui: &Ui, rect: Rect, name: &str, logo: Option<&[u8]>) {
    let painter = ui.painter();
    painter.rect_filled(rect, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SELECTED);
    if let Some(texture) = texture(ui.ctx(), Id::new("organization-logo-texture").with(ui.id()), logo) {
        painter.image(texture.id(), rect.shrink(2.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    } else {
        let initial: String = name.trim().chars().next().into_iter().flat_map(char::to_uppercase).collect();
        painter.text(rect.center(), Align2::CENTER_CENTER, initial, theme::font_body_strong(), theme::TEXT);
    }
}

pub(crate) fn field(ui: &mut Ui, origin: Pos2, name: &str, logo: &mut Option<Vec<u8>>, app: &mut App) {
    ui.painter().text(origin + Vec2::new(0.0, 9.0), Align2::LEFT_CENTER, "Organization logo (optional)", theme::font_body(), theme::TEXT);
    paint(ui, Rect::from_min_size(origin + Vec2::new(0.0, 22.0), Vec2::splat(30.0)), name, logo.as_deref());
    let label = if logo.is_some() { "Replace logo" } else { "Choose logo" };
    if ui.put(Rect::from_min_size(origin + Vec2::new(40.0, 22.0), Vec2::new(105.0, 30.0)), egui::Button::new(label)).clicked() {
        if let Some(path) = rfd::FileDialog::new().add_filter("Logo image", &["png", "jpg", "jpeg", "webp"]).pick_file() {
            match load(&path) {
                Ok(pixels) => *logo = Some(pixels),
                Err(error) => app.show_toast(format!("Cannot load logo: {error}"), true),
            }
        }
    }
    if logo.is_some()
        && ui.put(Rect::from_min_size(origin + Vec2::new(153.0, 22.0), Vec2::new(105.0, 30.0)), egui::Button::new("Remove logo")).clicked()
    {
        *logo = None;
    }
}
