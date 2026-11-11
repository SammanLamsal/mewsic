use std::path::PathBuf;

use eframe::egui;

use crate::app::Action;
use crate::settings::Settings;

/// Edits `draft`, a copy of the saved settings that only takes effect on Save.
pub fn show(ui: &mut egui::Ui, draft: &mut Settings, saved: &Settings, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.heading("Settings");
            ui.add_space(8.0);

            show_library_section(ui, draft);

            ui.add_space(12.0);
            show_save_buttons(ui, draft, saved, actions);
        });
}

fn show_library_section(ui: &mut egui::Ui, draft: &mut Settings) {
    ui.strong("Library");
    ui.label("Music folder");
    ui.weak("MPD's music folder on this computer. ReplayGain scans write their tags to the files in it.");

    let mut path = match &draft.music_dir {
        Some(dir) => dir.display().to_string(),
        None => String::new(),
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut path)
            .hint_text("Path to the music folder")
            .desired_width(f32::INFINITY),
    );
    if response.changed() {
        draft.music_dir = if path.is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        };
    }

    if let Some(dir) = &draft.music_dir
        && !dir.is_dir()
    {
        ui.colored_label(ui.visuals().error_fg_color, "That folder does not exist");
    }
}

fn show_save_buttons(
    ui: &mut egui::Ui,
    draft: &mut Settings,
    saved: &Settings,
    actions: &mut Vec<Action>,
) {
    let has_changes = draft != saved;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(has_changes, egui::Button::new("Save"))
            .clicked()
        {
            actions.push(Action::SaveSettings(draft.clone()));
        }
        if ui
            .add_enabled(has_changes, egui::Button::new("Revert"))
            .clicked()
        {
            *draft = saved.clone();
        }
    });
}
