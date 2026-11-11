pub mod browser;
pub mod dialogs;
pub mod keys;
pub mod motion;
pub mod nowplaying;
pub mod settings;
pub mod tabs;
pub mod tracks;
pub mod transport;

use std::time::Duration;

use eframe::egui;

use crate::app::Action;
use crate::replaygain::ScanMode;

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

/// Marks the pane that keyboard commands go to.
pub fn outline_focused_pane(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.0, ui.visuals().selection.bg_fill);
    ui.painter()
        .rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
}

/// The "Scan ReplayGain" submenu shared by the track and sidebar context menus.
/// `uris` is only called once a choice is clicked.
pub fn replay_gain_menu(
    ui: &mut egui::Ui,
    uris: impl Fn() -> Vec<String>,
    actions: &mut Vec<Action>,
) {
    ui.menu_button("Scan ReplayGain", |ui| {
        let choices = [
            (
                ScanMode::ByAlbum,
                "By album",
                "Album gain is measured over the chosen tracks of each album",
            ),
            (
                ScanMode::TrackByTrack,
                "Track by track",
                "Writes only track gain, and removes any album gain the files had",
            ),
        ];
        for (mode, label, tooltip) in choices {
            if ui.button(label).on_hover_text(tooltip).clicked() {
                actions.push(Action::ScanReplayGain { uris: uris(), mode });
            }
        }
    });
}

/// Whether a mouse button went down inside `rect` this frame.
pub fn pressed_inside(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|input| {
        if !input.pointer.any_pressed() {
            return false;
        }
        match input.pointer.interact_pos() {
            Some(position) => rect.contains(position),
            None => false,
        }
    })
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
