use eframe::egui;

use crate::app::Action;
use crate::art::ArtCache;
use crate::library::Song;
use crate::mpd::MpdHandle;

/// How wide the panel may grow when there is no album art to size it by.
const WIDTH_WITHOUT_ART: f32 = 320.0;

/// The widest the panel's content gets: the album art at its real size.
pub fn content_width(song: Option<&Song>, art: &ArtCache) -> f32 {
    match song.and_then(|song| art.loaded_size(song)) {
        Some(art_size) => art_size.x,
        None => WIDTH_WITHOUT_ART,
    }
}

pub fn show(
    ui: &mut egui::Ui,
    song: Option<&Song>,
    art: &mut ArtCache,
    mpd: &mut MpdHandle,
    actions: &mut Vec<Action>,
) {
    let Some(song) = song else {
        ui.centered_and_justified(|ui| {
            ui.weak("Nothing playing");
        });
        return;
    };

    let size = match art.loaded_size(song) {
        Some(art_size) => ui.available_width().min(art_size.x),
        None => ui.available_width(),
    };
    album_art(ui, song, size, art, mpd);
    ui.add_space(8.0);

    ui.add(egui::Label::new(egui::RichText::new(song.display_title()).heading()).wrap());
    if !song.artist.is_empty() {
        ui.label(&song.artist);
    }
    if !song.album.is_empty() {
        ui.weak(&song.album);
    }
    if !song.date.is_empty() {
        ui.weak(&song.date);
    }

    ui.add_space(8.0);
    if ui
        .button("Show in queue")
        .on_hover_text("Scroll to the playing track (o)")
        .clicked()
    {
        actions.push(Action::ShowPlaying);
    }
}

pub fn album_art(
    ui: &mut egui::Ui,
    song: &Song,
    size: f32,
    art: &mut ArtCache,
    mpd: &mut MpdHandle,
) {
    let is_loading = art.is_loading(song);
    match art.get(song, mpd) {
        Some(texture) => {
            let image = egui::Image::new(texture)
                .max_size(egui::vec2(size, size))
                .corner_radius(4.0);
            ui.add(image);
        }
        None => placeholder(ui, size, is_loading),
    }
}

fn placeholder(ui: &mut egui::Ui, size: f32, is_loading: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let visuals = ui.visuals();
    ui.painter().rect_filled(rect, 4.0, visuals.faint_bg_color);

    if is_loading {
        let spinner_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(24.0, 24.0));
        ui.put(spinner_rect, egui::Spinner::new());
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "♪",
            egui::FontId::proportional(size * 0.3),
            visuals.weak_text_color(),
        );
    }
}
