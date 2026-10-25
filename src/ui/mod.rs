pub mod browser;
pub mod dialogs;
pub mod nowplaying;
pub mod settings;
pub mod tabs;
pub mod tracks;
pub mod transport;

use std::time::Duration;

use eframe::egui;

pub fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Width of `text` in the body font, measured without laying it out.
pub fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    ui.ctx()
        .fonts_mut(|fonts| text.chars().map(|c| fonts.glyph_width(&font_id, c)).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(format_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn bundled_fonts_have_the_icons_we_use() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let font_id = egui::FontId::proportional(14.0);
            let has_icons = ui
                .ctx()
                .fonts_mut(|fonts| fonts.has_glyphs(&font_id, "⏮⏸⏭▶⏶⏷🔀🔁🔂🔊🗑♪×…·—⚙⟳"));
            assert!(has_icons);
        });
        output.textures_delta.clear();
    }
}
