//! The interface's colours, fonts and art.
use egui::*;

// re5OSX's colours, shared with its setup window (crates/re5osx-launcher/web).
pub(super) const INK: Color32 = Color32::from_rgb(10, 10, 10);
pub(super) const MUTED: Color32 = Color32::from_rgb(176, 176, 176);
pub(super) const GOLD: Color32 = Color32::from_rgb(252, 196, 25);

pub(super) fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name("tile".into())))
}

/// The game's menu font (Chalet London 1960), which re5OSX's launcher also uses for headings: `b/<build>/title/chalet.ttf`
/// in the game folder named on the command line, in `RE5OSX_GAME_DIR` or in a default place. Not bundled; Barlow when absent.
fn chalet() -> Option<&'static [u8]> {
    static FONT: std::sync::OnceLock<Option<&'static [u8]>> = std::sync::OnceLock::new();
    *FONT.get_or_init(|| {
        let dirs = std::env::args_os().skip(1).map(std::path::PathBuf::from)
            .chain(std::env::var_os("RE5OSX_GAME_DIR").map(std::path::PathBuf::from))
            .chain(crate::paths::default_game_dirs());
        for dir in dirs {
            let Ok(builds) = std::fs::read_dir(dir.join("b")) else { continue };
            for build in builds.flatten() {
                if let Ok(bytes) = std::fs::read(build.path().join("title").join("chalet.ttf")) {
                    return Some(&*Box::leak(bytes.into_boxed_slice()));
                }
            }
        }
        None
    })
}

pub(super) fn install(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("ui", include_bytes!("../../assets/ui/Barlow-SemiBold.ttf").as_slice()),
        ("condensed", include_bytes!("../../assets/ui/BarlowCondensed-Bold.ttf").as_slice()),
    ] {
        fonts.font_data.insert(name.into(), FontData::from_static(bytes).into());
    }
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    let mut headings = vec!["condensed".into()];
    if let Some(bytes) = chalet() {
        fonts.font_data.insert("chalet".into(), FontData::from_static(bytes).into());
        headings.insert(0, "chalet".into());
    }
    headings.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts.families.insert(FontFamily::Name("condensed".into()), headings);
    // Tile and tab titles: bold condensed capitals, as in re5OSX's launcher and the game's menu.
    let mut tiles = vec!["condensed".into()];
    tiles.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts.families.insert(FontFamily::Name("tile".into()), tiles);
    // Running text as re5OSX's launcher sets it (-apple-system): the system's own face where there is one (macOS's San
    // Francisco, regular weight), else the UI's.
    let mut body = Vec::new();
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        fonts.font_data.insert("system".into(), FontData::from_owned(bytes).into());
        body.push("system".into());
    }
    body.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts.families.insert(FontFamily::Name("body".into()), body);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|s| {
        s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(13.0));
        s.spacing.item_spacing = vec2(10.0, 10.0);
        s.spacing.button_padding = vec2(12.0, 8.0);
        s.spacing.interact_size.y = 44.0;
        s.visuals.override_text_color = Some(Color32::WHITE);
        s.visuals.panel_fill = INK;
        s.visuals.window_fill = Color32::from_rgba_unmultiplied(20, 20, 20, 245);
        s.visuals.window_corner_radius = 6.into();
        s.visuals.window_stroke = Stroke::new(1.0, Color32::from_gray(51));
        s.visuals.selection.bg_fill = Color32::from_rgb(70, 50, 10);
        s.visuals.selection.stroke = Stroke::new(1.0, GOLD);
        for w in [&mut s.visuals.widgets.inactive, &mut s.visuals.widgets.hovered, &mut s.visuals.widgets.active] {
            w.corner_radius = 4.into();
            w.bg_fill = Color32::from_gray(31);
            w.weak_bg_fill = Color32::from_gray(26);
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        }
        s.visuals.widgets.hovered.bg_fill = Color32::from_gray(46);
        s.visuals.widgets.active.bg_stroke = Stroke::new(2.0, GOLD);
    });
}

/// Density follows available space, including landscape phones and split views.
pub(super) fn layout_scale(rect: Rect) -> f32 {
    (rect.height() / 560.0).min(rect.width() / 780.0).clamp(0.75, 1.0)
}

pub(super) fn density(ui: &Ui) -> f32 {
    ui.style().text_styles[&TextStyle::Body].size / 14.0
}

pub(super) fn apply_density(ui: &mut Ui, scale: f32) {
    let style = ui.style_mut();
    for font in style.text_styles.values_mut() {
        font.size *= scale;
    }
    style.spacing.item_spacing *= scale;
    style.spacing.button_padding *= scale;
    style.spacing.interact_size *= scale;
}



/// A picture for the start page's tiles (PNG, from assets/ui/tiles), decoded once.
pub(super) fn art(ctx: &Context, name: &'static str) -> Option<TextureHandle> {
    let id = Id::new(("tile-art", name));
    if let Some(texture) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return texture;
    }
    let path = crate::paths::art_dir().join(format!("{name}.png"));
    let Ok(bytes) = std::fs::read(&path) else {
        ctx.data_mut(|d| d.insert_temp(id, None::<TextureHandle>));
        return None;
    };
    let decode = || -> Result<ColorImage, png::DecodingError> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut pixels)?;
        let size = [info.width as usize, info.height as usize];
        Ok(match info.color_type {
            png::ColorType::Rgb => ColorImage::from_rgb(size, &pixels[..info.buffer_size()]),
            _ => ColorImage::from_rgba_unmultiplied(size, &pixels[..info.buffer_size()]),
        })
    };
    let texture = match decode() {
        Ok(image) => Some(ctx.load_texture(name, image, TextureOptions::LINEAR)),
        Err(error) => {
            log::warn!("Could not load tile art {name}: {error}");
            None
        }
    };
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}
