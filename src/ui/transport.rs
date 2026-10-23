use std::time::Duration;

use eframe::egui;

use crate::app::{Action, ConnectionState};
use crate::mpd::{
    Command, LoopMode, PlayState, PlayerStatus, REPLAY_GAIN_MODES, ReplayGainMode,
    replay_gain_label,
};
use crate::ui::format_duration;

/// The seek position is only sent to MPD when the drag ends.
#[derive(Default)]
pub struct TransportState {
    seek_preview: Option<f32>,
}

pub struct TransportView<'a> {
    pub status: &'a PlayerStatus,
    pub replay_gain: ReplayGainMode,
    pub connection: &'a ConnectionState,
}

pub fn show(
    ui: &mut egui::Ui,
    view: TransportView,
    state: &mut TransportState,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        show_playback_buttons(ui, view.status, actions);
        show_mode_buttons(ui, view.status, actions);
        ui.separator();
        show_seek_bar(ui, view.status, state, actions);
        ui.separator();
        show_volume(ui, view.status, actions);
        ui.separator();
        show_replay_gain(ui, view.replay_gain, actions);
        ui.separator();
        show_connection(ui, view.connection);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("×").on_hover_text("Quit mewsic").clicked() {
                actions.push(Action::Quit);
            }
        });
    });
}

fn show_mode_buttons(ui: &mut egui::Ui, status: &PlayerStatus, actions: &mut Vec<Action>) {
    if ui
        .selectable_label(status.random, "🔀")
        .on_hover_text("Shuffle (random order)")
        .clicked()
    {
        actions.push(Action::Mpd(Command::SetRandom(!status.random)));
    }

    let (label, tooltip) = match status.loop_mode {
        LoopMode::Off => ("🔁", "Loop: off"),
        LoopMode::Queue => ("🔁", "Loop: queue"),
        LoopMode::Track => ("🔂", "Loop: track"),
    };
    if ui
        .selectable_label(status.loop_mode != LoopMode::Off, label)
        .on_hover_text(tooltip)
        .clicked()
    {
        actions.push(Action::Mpd(Command::SetLoop(status.loop_mode.next())));
    }
}

fn show_playback_buttons(ui: &mut egui::Ui, status: &PlayerStatus, actions: &mut Vec<Action>) {
    if ui.button("⏮").on_hover_text("Previous").clicked() {
        actions.push(Action::Mpd(Command::Previous));
    }

    let play_pause_label = if status.state == PlayState::Playing {
        "⏸"
    } else {
        "▶"
    };
    if ui
        .button(play_pause_label)
        .on_hover_text("Play / pause (Space)")
        .clicked()
    {
        actions.push(Action::TogglePlayback);
    }

    if ui.button("⏭").on_hover_text("Next").clicked() {
        actions.push(Action::Mpd(Command::Next));
    }
}

fn show_seek_bar(
    ui: &mut egui::Ui,
    status: &PlayerStatus,
    state: &mut TransportState,
    actions: &mut Vec<Action>,
) {
    let duration = status.duration.as_secs_f32();
    let mut position = match state.seek_preview {
        Some(preview) => preview,
        None => status.elapsed.as_secs_f32(),
    };

    ui.label(format!(
        "{} / {}",
        format_duration(Duration::from_secs_f32(position)),
        format_duration(status.duration)
    ));

    let reserved_for_other_controls = 460.0;
    ui.spacing_mut().slider_width = (ui.available_width() - reserved_for_other_controls).max(80.0);

    let slider = egui::Slider::new(&mut position, 0.0..=duration.max(0.1)).show_value(false);
    let response = ui.add_enabled(duration > 0.0, slider);

    if response.dragged() {
        state.seek_preview = Some(position);
    } else if response.drag_stopped() || response.changed() {
        state.seek_preview = None;
        actions.push(Action::Mpd(Command::SeekTo(Duration::from_secs_f32(
            position,
        ))));
    }
}

fn show_volume(ui: &mut egui::Ui, status: &PlayerStatus, actions: &mut Vec<Action>) {
    ui.label("🔊");
    ui.spacing_mut().slider_width = 90.0;
    let mut volume = status.volume;
    let response = ui.add(egui::Slider::new(&mut volume, 0..=100).show_value(false));
    if response.changed() {
        actions.push(Action::Mpd(Command::SetVolume(volume)));
    }
}

fn show_replay_gain(ui: &mut egui::Ui, current: ReplayGainMode, actions: &mut Vec<Action>) {
    egui::ComboBox::from_label("ReplayGain")
        .selected_text(replay_gain_label(current))
        .show_ui(ui, |ui| {
            for mode in REPLAY_GAIN_MODES {
                let selected = mode == current;
                if ui
                    .selectable_label(selected, replay_gain_label(mode))
                    .clicked()
                    && !selected
                {
                    actions.push(Action::Mpd(Command::SetReplayGain(mode)));
                }
            }
        });
}

fn show_connection(ui: &mut egui::Ui, connection: &ConnectionState) {
    let (color, tooltip) = match connection {
        ConnectionState::Connecting => (egui::Color32::GRAY, "Connecting to MPD…".to_string()),
        ConnectionState::Connected => (
            egui::Color32::from_rgb(80, 180, 100),
            "Connected".to_string(),
        ),
        ConnectionState::Disconnected(reason) => {
            (egui::Color32::from_rgb(210, 80, 80), reason.clone())
        }
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, color);
    response.on_hover_text(tooltip);
}
