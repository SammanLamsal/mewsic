use std::path::Path;

use eframe::egui;

pub enum Dialog {
    NewQueue {
        name: String,
        tracks: Vec<String>,
    },
    RenameQueue {
        tab: usize,
        name: String,
    },
    ConfirmDelete {
        name: String,
    },
    /// Asked for the first time a ReplayGain scan needs the files; the scan starts once confirmed.
    MusicFolder {
        path: String,
        pending_scan: Vec<String>,
    },
}

#[derive(PartialEq)]
pub enum DialogOutcome {
    StillOpen,
    Cancelled,
    Confirmed,
}

pub fn show(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    is_name_available: impl Fn(&str) -> bool,
) -> DialogOutcome {
    let modal = egui::Modal::new(egui::Id::unique("mewsic_dialog")).show(ctx, |ui| {
        ui.set_width(320.0);
        match dialog {
            Dialog::NewQueue { name, tracks } => {
                let detail = match tracks.len() {
                    0 => "Empty queue".to_string(),
                    1 => "1 track".to_string(),
                    count => format!("{count} tracks"),
                };
                name_prompt(ui, "New queue", &detail, name, &is_name_available)
            }
            Dialog::RenameQueue { name, .. } => {
                name_prompt(ui, "Rename queue", "", name, &is_name_available)
            }
            Dialog::ConfirmDelete { name } => confirm_delete(ui, name),
            Dialog::MusicFolder { path, .. } => music_folder_prompt(ui, path),
        }
    });

    if modal.inner == DialogOutcome::StillOpen && modal.should_close() {
        return DialogOutcome::Cancelled;
    }
    modal.inner
}

fn name_prompt(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    name: &mut String,
    is_name_available: &impl Fn(&str) -> bool,
) -> DialogOutcome {
    ui.heading(title);
    if !detail.is_empty() {
        ui.weak(detail);
    }
    ui.add_space(4.0);

    let response = ui.add(egui::TextEdit::singleline(name).hint_text("Queue name"));
    if ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }

    let trimmed = name.trim();
    let is_valid = !trimmed.is_empty() && is_name_available(trimmed);
    if !trimmed.is_empty() && !is_valid {
        ui.colored_label(ui.visuals().error_fg_color, "That name is already taken");
    }

    let pressed_enter =
        response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let mut outcome = DialogOutcome::StillOpen;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let ok = ui.add_enabled(is_valid, egui::Button::new("OK"));
        if ok.clicked() || (pressed_enter && is_valid) {
            outcome = DialogOutcome::Confirmed;
        }
        if ui.button("Cancel").clicked() {
            outcome = DialogOutcome::Cancelled;
        }
    });
    outcome
}

fn music_folder_prompt(ui: &mut egui::Ui, path: &mut String) -> DialogOutcome {
    ui.heading("Music folder");
    ui.label(
        "ReplayGain tags are written to the files themselves, so mewsic needs MPD's music folder.",
    );
    ui.weak("You can change it later in ⚙ Settings.");
    ui.add_space(4.0);

    let response = ui.add(
        egui::TextEdit::singleline(path)
            .hint_text("Path to the music folder")
            .desired_width(f32::INFINITY),
    );
    if ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }

    let trimmed = path.trim();
    let is_valid = Path::new(trimmed).is_dir();
    if !trimmed.is_empty() && !is_valid {
        ui.colored_label(ui.visuals().error_fg_color, "That folder does not exist");
    }

    let pressed_enter =
        response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let mut outcome = DialogOutcome::StillOpen;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let ok = ui.add_enabled(is_valid, egui::Button::new("Scan"));
        if ok.clicked() || (pressed_enter && is_valid) {
            outcome = DialogOutcome::Confirmed;
        }
        if ui.button("Cancel").clicked() {
            outcome = DialogOutcome::Cancelled;
        }
    });
    outcome
}

fn confirm_delete(ui: &mut egui::Ui, name: &str) -> DialogOutcome {
    ui.heading("Delete queue");
    ui.label(format!(
        "Delete the saved queue \"{name}\"? This cannot be undone."
    ));
    ui.add_space(4.0);

    let mut outcome = DialogOutcome::StillOpen;
    ui.horizontal(|ui| {
        if ui.button("Delete").clicked() {
            outcome = DialogOutcome::Confirmed;
        }
        if ui.button("Cancel").clicked() {
            outcome = DialogOutcome::Cancelled;
        }
    });
    outcome
}
