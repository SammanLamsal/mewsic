use eframe::egui::{self, Key, Modifiers};

use crate::app::Pane;
use crate::ui::browser::BrowserCommand;
use crate::ui::motion::Motion;
use crate::ui::tracks::TrackCommand;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyCommand {
    FocusFilter,
    FocusBrowserSearch,
    FocusLeftPane,
    FocusRightPane,
    ResizeLeft,
    ResizeRight,
    TogglePlayback,
    PreviousTrack,
    NextTrack,
    SeekBackward,
    SeekForward,
    VolumeDown,
    VolumeUp,
    ToggleShuffle,
    CycleLoop,
    ClearSelection,
    RemoveSelected,
    NextTab,
    PreviousTab,
    NextBrowseMode,
    PreviousBrowseMode,
    ShowPlaying,
    Track(TrackCommand),
    Browser(BrowserCommand),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Prefix {
    G,
    D,
}

/// The first key of a sequence such as `gg`, while it waits for the second.
#[derive(Default)]
pub struct KeyState {
    pending_prefix: Option<Prefix>,
}

type Binding = (Modifiers, Key, KeyCommand);

const fn move_cursor(motion: Motion, extend: bool) -> KeyCommand {
    KeyCommand::Track(TrackCommand::Move { motion, extend })
}

const fn browse(motion: Motion) -> KeyCommand {
    KeyCommand::Browser(BrowserCommand::Move(motion))
}

/// Keys that work whichever pane has focus.
const COMMON_KEYS: [Binding; 13] = [
    (Modifiers::CTRL, Key::H, KeyCommand::FocusLeftPane),
    (Modifiers::CTRL, Key::L, KeyCommand::FocusRightPane),
    (Modifiers::ALT, Key::H, KeyCommand::ResizeLeft),
    (Modifiers::ALT, Key::L, KeyCommand::ResizeRight),
    (Modifiers::NONE, Key::Space, KeyCommand::TogglePlayback),
    (Modifiers::NONE, Key::OpenBracket, KeyCommand::PreviousTrack),
    (Modifiers::NONE, Key::CloseBracket, KeyCommand::NextTrack),
    (Modifiers::NONE, Key::Minus, KeyCommand::VolumeDown),
    (Modifiers::NONE, Key::Plus, KeyCommand::VolumeUp),
    (Modifiers::NONE, Key::Equals, KeyCommand::VolumeUp),
    (Modifiers::NONE, Key::S, KeyCommand::ToggleShuffle),
    (Modifiers::NONE, Key::R, KeyCommand::CycleLoop),
    (Modifiers::NONE, Key::O, KeyCommand::ShowPlaying),
];

/// egui ignores an extra Shift when matching, so Shift bindings must come before
/// the plain binding of the same key.
const TRACK_KEYS: [Binding; 18] = [
    (Modifiers::SHIFT, Key::J, move_cursor(Motion::Down, true)),
    (Modifiers::SHIFT, Key::K, move_cursor(Motion::Up, true)),
    (
        Modifiers::SHIFT,
        Key::ArrowDown,
        move_cursor(Motion::Down, true),
    ),
    (
        Modifiers::SHIFT,
        Key::ArrowUp,
        move_cursor(Motion::Up, true),
    ),
    (Modifiers::SHIFT, Key::G, move_cursor(Motion::Last, false)),
    (Modifiers::NONE, Key::J, move_cursor(Motion::Down, false)),
    (Modifiers::NONE, Key::K, move_cursor(Motion::Up, false)),
    (
        Modifiers::NONE,
        Key::ArrowDown,
        move_cursor(Motion::Down, false),
    ),
    (
        Modifiers::NONE,
        Key::ArrowUp,
        move_cursor(Motion::Up, false),
    ),
    (
        Modifiers::CTRL,
        Key::D,
        move_cursor(Motion::HalfPageDown, false),
    ),
    (
        Modifiers::CTRL,
        Key::U,
        move_cursor(Motion::HalfPageUp, false),
    ),
    (
        Modifiers::NONE,
        Key::Enter,
        KeyCommand::Track(TrackCommand::PlayCursor),
    ),
    (
        Modifiers::COMMAND,
        Key::A,
        KeyCommand::Track(TrackCommand::SelectAll),
    ),
    (Modifiers::NONE, Key::Slash, KeyCommand::FocusFilter),
    (Modifiers::NONE, Key::Escape, KeyCommand::ClearSelection),
    (Modifiers::NONE, Key::Delete, KeyCommand::RemoveSelected),
    (Modifiers::NONE, Key::H, KeyCommand::SeekBackward),
    (Modifiers::NONE, Key::L, KeyCommand::SeekForward),
];

/// The sidebar has a single selection, so Shift+J moves just like `j`.
const BROWSER_KEYS: [Binding; 11] = [
    (Modifiers::SHIFT, Key::G, browse(Motion::Last)),
    (Modifiers::NONE, Key::J, browse(Motion::Down)),
    (Modifiers::NONE, Key::K, browse(Motion::Up)),
    (Modifiers::NONE, Key::ArrowDown, browse(Motion::Down)),
    (Modifiers::NONE, Key::ArrowUp, browse(Motion::Up)),
    (Modifiers::CTRL, Key::D, browse(Motion::HalfPageDown)),
    (Modifiers::CTRL, Key::U, browse(Motion::HalfPageUp)),
    (
        Modifiers::NONE,
        Key::Enter,
        KeyCommand::Browser(BrowserCommand::Play),
    ),
    (Modifiers::NONE, Key::Slash, KeyCommand::FocusBrowserSearch),
    (
        Modifiers::NONE,
        Key::H,
        KeyCommand::Browser(BrowserCommand::Collapse),
    ),
    (
        Modifiers::NONE,
        Key::L,
        KeyCommand::Browser(BrowserCommand::Expand),
    ),
];

fn pane_keys(pane: Pane) -> &'static [Binding] {
    match pane {
        Pane::Browser => &BROWSER_KEYS,
        Pane::Tracks => &TRACK_KEYS,
        Pane::NowPlaying => &[],
    }
}

/// Keys that work even while typing in a text field.
pub fn read_global(input: &mut egui::InputState) -> Option<KeyCommand> {
    if input.consume_key(Modifiers::COMMAND, Key::F) {
        return Some(KeyCommand::FocusFilter);
    }
    None
}

/// A pending prefix is dropped by any key that doesn't complete it, and that key then works as usual.
pub fn read(input: &mut egui::InputState, state: &mut KeyState, pane: Pane) -> Vec<KeyCommand> {
    if let Some(prefix) = state.pending_prefix {
        if !any_key_pressed(input) {
            return Vec::new();
        }
        state.pending_prefix = None;
        if let Some(command) = read_after_prefix(input, prefix, pane) {
            return vec![command];
        }
    }

    // egui also ignores an extra Alt when matching, so the common keys go first
    // for Alt+H to win over a pane's plain `h`.
    let mut commands = Vec::new();
    consume_bindings(input, &COMMON_KEYS, &mut commands);
    consume_bindings(input, pane_keys(pane), &mut commands);

    if input.consume_key(Modifiers::NONE, Key::G) {
        state.pending_prefix = Some(Prefix::G);
    } else if pane == Pane::Tracks && input.consume_key(Modifiers::NONE, Key::D) {
        state.pending_prefix = Some(Prefix::D);
    }
    commands
}

fn consume_bindings(
    input: &mut egui::InputState,
    bindings: &[Binding],
    commands: &mut Vec<KeyCommand>,
) {
    for &(modifiers, key, command) in bindings {
        if input.consume_key(modifiers, key) {
            commands.push(command);
        }
    }
}

fn read_after_prefix(
    input: &mut egui::InputState,
    prefix: Prefix,
    pane: Pane,
) -> Option<KeyCommand> {
    match prefix {
        Prefix::G => {
            if input.consume_key(Modifiers::SHIFT, Key::T) {
                Some(previous_tab(pane))
            } else if input.consume_key(Modifiers::NONE, Key::T) {
                Some(next_tab(pane))
            } else if input.consume_key(Modifiers::NONE, Key::G) {
                jump_to_first(pane)
            } else {
                None
            }
        }
        Prefix::D => {
            if input.consume_key(Modifiers::NONE, Key::D) {
                Some(KeyCommand::RemoveSelected)
            } else {
                None
            }
        }
    }
}

/// In the sidebar, the tabs are its browse modes.
fn next_tab(pane: Pane) -> KeyCommand {
    match pane {
        Pane::Browser => KeyCommand::NextBrowseMode,
        Pane::Tracks | Pane::NowPlaying => KeyCommand::NextTab,
    }
}

fn previous_tab(pane: Pane) -> KeyCommand {
    match pane {
        Pane::Browser => KeyCommand::PreviousBrowseMode,
        Pane::Tracks | Pane::NowPlaying => KeyCommand::PreviousTab,
    }
}

fn jump_to_first(pane: Pane) -> Option<KeyCommand> {
    match pane {
        Pane::Browser => Some(browse(Motion::First)),
        Pane::Tracks => Some(move_cursor(Motion::First, false)),
        Pane::NowPlaying => None,
    }
}

fn any_key_pressed(input: &egui::InputState) -> bool {
    input
        .events
        .iter()
        .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Feeds each frame's key presses through `read`, returning every frame's commands.
    fn read_frames(pane: Pane, frames: Vec<Vec<egui::Event>>) -> Vec<Vec<KeyCommand>> {
        let ctx = egui::Context::default();
        let mut state = KeyState::default();
        let mut results = Vec::new();
        for events in frames {
            let raw_input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw_input, |ui| {
                results.push(ui.input_mut(|input| read(input, &mut state, pane)));
            });
            output.textures_delta.clear();
        }
        results
    }

    #[test]
    fn g_twice_jumps_to_the_first_track() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::G, Modifiers::NONE)],
                vec![press(Key::G, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[0], []);
        assert_eq!(results[1], [move_cursor(Motion::First, false)]);
    }

    #[test]
    fn an_unrelated_key_cancels_the_prefix_and_still_works() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::G, Modifiers::NONE)],
                vec![press(Key::J, Modifiers::NONE)],
                vec![press(Key::G, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[1], [move_cursor(Motion::Down, false)]);
        assert_eq!(results[2], []);
    }

    #[test]
    fn shift_bindings_win_over_plain_ones() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::G, Modifiers::SHIFT)],
                vec![press(Key::J, Modifiers::SHIFT)],
            ],
        );
        assert_eq!(results[0], [move_cursor(Motion::Last, false)]);
        assert_eq!(results[1], [move_cursor(Motion::Down, true)]);
    }

    #[test]
    fn d_twice_removes_and_ctrl_d_scrolls() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::D, Modifiers::NONE)],
                vec![press(Key::D, Modifiers::NONE)],
                vec![press(Key::D, Modifiers::CTRL)],
            ],
        );
        assert_eq!(results[1], [KeyCommand::RemoveSelected]);
        assert_eq!(results[2], [move_cursor(Motion::HalfPageDown, false)]);
    }

    #[test]
    fn g_then_shift_t_goes_to_the_previous_tab() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::G, Modifiers::NONE)],
                vec![press(Key::T, Modifiers::SHIFT)],
            ],
        );
        assert_eq!(results[1], [KeyCommand::PreviousTab]);
    }

    #[test]
    fn ctrl_and_alt_h_move_focus_and_resize_instead_of_seeking() {
        let results = read_frames(
            Pane::Tracks,
            vec![
                vec![press(Key::H, Modifiers::CTRL)],
                vec![press(Key::H, Modifiers::ALT)],
                vec![press(Key::H, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[0], [KeyCommand::FocusLeftPane]);
        assert_eq!(results[1], [KeyCommand::ResizeLeft]);
        assert_eq!(results[2], [KeyCommand::SeekBackward]);
    }

    #[test]
    fn brackets_skip_tracks_in_every_pane() {
        let results = read_frames(
            Pane::NowPlaying,
            vec![
                vec![press(Key::OpenBracket, Modifiers::NONE)],
                vec![press(Key::CloseBracket, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[0], [KeyCommand::PreviousTrack]);
        assert_eq!(results[1], [KeyCommand::NextTrack]);
    }

    #[test]
    fn o_shows_the_playing_track_from_every_pane() {
        for pane in [Pane::Browser, Pane::Tracks, Pane::NowPlaying] {
            let results = read_frames(pane, vec![vec![press(Key::O, Modifiers::NONE)]]);
            assert_eq!(results[0], [KeyCommand::ShowPlaying]);
        }
    }

    #[test]
    fn the_sidebar_has_its_own_keys() {
        let results = read_frames(
            Pane::Browser,
            vec![
                vec![press(Key::J, Modifiers::NONE)],
                vec![press(Key::H, Modifiers::NONE)],
                vec![press(Key::G, Modifiers::NONE)],
                vec![press(Key::T, Modifiers::NONE)],
                vec![press(Key::Slash, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[0], [browse(Motion::Down)]);
        assert_eq!(results[1], [KeyCommand::Browser(BrowserCommand::Collapse)]);
        assert_eq!(results[3], [KeyCommand::NextBrowseMode]);
        assert_eq!(results[4], [KeyCommand::FocusBrowserSearch]);
    }

    #[test]
    fn d_twice_does_nothing_in_the_sidebar() {
        let results = read_frames(
            Pane::Browser,
            vec![
                vec![press(Key::D, Modifiers::NONE)],
                vec![press(Key::D, Modifiers::NONE)],
            ],
        );
        assert_eq!(results[0], []);
        assert_eq!(results[1], []);
    }
}
