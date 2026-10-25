use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::art::ArtCache;
use crate::filter::Query;
use crate::library::{Collection, Library, Song, SongSort, SortColumn};
use crate::mpd::{
    Command, ConnectionConfig, Event, MpdHandle, PlayState, PlayerStatus, ReplayGainMode,
};
use crate::queues::{self, OpenQueues, QueueStore, Session};
use crate::replaygain::{self, ReplayGainScan};
use crate::settings::{Settings, SettingsStore};
use crate::ui::browser::{self, BrowseMode, BrowserSearch, BrowserView};
use crate::ui::dialogs::{self, Dialog, DialogOutcome};
use crate::ui::tracks::{self, ColumnWidths, Selection, TrackRow, TrackSource, TrackTable};
use crate::ui::transport::{self, TransportState, TransportView};
use crate::ui::{nowplaying, settings, tabs};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const EXIT_STOP_TIMEOUT: Duration = Duration::from_secs(1);
const FILTER_ID: &str = "track_filter";
const MIN_PANEL_WIDTH: f32 = 160.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Library,
    Queue(usize),
    Settings,
}

pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected(String),
}

/// Everything the UI can ask for. Widgets only push actions; `App::apply` carries them out.
pub enum Action {
    Mpd(Command),
    TogglePlayback,
    ShowView(View),
    SetBrowseMode(BrowseMode),
    SelectCollection(Collection),
    PlayCollection(Collection),
    AddCollectionToQueue {
        tab: usize,
        collection: Collection,
    },
    AskNewQueueFromCollection(Collection),
    SortBy(SortColumn),
    PlayNow(Vec<String>),
    AddToQueue {
        tab: usize,
        uris: Vec<String>,
    },
    AskNewQueue {
        name_hint: String,
        tracks: Vec<String>,
    },
    PlayInTab {
        tab: usize,
        position: usize,
    },
    ResumeTab(usize),
    RemoveFromTab {
        tab: usize,
        positions: Vec<usize>,
    },
    MoveInTab {
        tab: usize,
        from: usize,
        to: usize,
    },
    CloseTab(usize),
    AskRenameTab(usize),
    OpenSavedQueue(String),
    AskDeleteSavedQueue(String),
    ScanReplayGain(Vec<String>),
    ScanCollectionReplayGain(Collection),
    SaveSettings(Settings),
    Quit,
}

pub struct App {
    mpd: MpdHandle,
    connection: ConnectionState,
    status: PlayerStatus,
    replay_gain: ReplayGainMode,
    library: Library,
    art: ArtCache,

    store: QueueStore,
    queues: OpenQueues,
    saved_session: Session,
    autosave_due: Option<Instant>,

    settings_store: SettingsStore,
    settings: Settings,
    settings_draft: Settings,
    replay_gain_scan: Option<ReplayGainScan>,

    view: View,
    browse_mode: BrowseMode,
    collection: Collection,
    browser_search: BrowserSearch,
    browser_width: Option<f32>,
    column_widths: ColumnWidths,
    library_sort: Option<SongSort>,
    filter: String,
    focus_filter: bool,
    /// Set for one frame by Ctrl+A. The track views apply it, since they know which rows the filter leaves visible.
    select_all_tracks: bool,
    selection: Selection,
    transport: TransportState,
    dialog: Option<Dialog>,
    error: Option<String>,
}

impl App {
    pub fn new(
        ctx: &egui::Context,
        config: ConnectionConfig,
        config_dir: &Path,
        data_dir: &Path,
        cache_dir: &Path,
    ) -> App {
        let settings_store = SettingsStore::new(config_dir);
        let settings = settings_store.load();
        let store = QueueStore::new(data_dir);
        let saved_session = store.load_session();

        let mut errors = Vec::new();
        let mut restored = Vec::new();
        for name in &saved_session.open_queues {
            match store.load(name) {
                Ok(queue) => restored.push(queue),
                Err(error) => errors.push(format!("{error:#}")),
            }
        }

        let mut queues = OpenQueues::default();
        queues.restore(restored, saved_session.active_queue.clone());

        let mpd = MpdHandle::spawn(config, cache_dir.join("art"), ctx.clone());

        App {
            mpd,
            connection: ConnectionState::Connecting,
            status: PlayerStatus::default(),
            replay_gain: ReplayGainMode::Off,
            library: Library::default(),
            art: ArtCache::default(),
            store,
            queues,
            saved_session,
            autosave_due: None,
            settings_store,
            settings_draft: settings.clone(),
            settings,
            replay_gain_scan: None,
            view: View::Library,
            browse_mode: BrowseMode::Folders,
            collection: Collection::All,
            browser_search: BrowserSearch::default(),
            browser_width: None,
            column_widths: ColumnWidths::default(),
            library_sort: None,
            filter: String::new(),
            focus_filter: false,
            select_all_tracks: false,
            selection: Selection::default(),
            transport: TransportState::default(),
            dialog: None,
            error: errors.into_iter().next(),
        }
    }

    fn handle_mpd_events(&mut self, ctx: &egui::Context) {
        for event in self.mpd.poll_events() {
            match event {
                Event::Connected => {
                    self.connection = ConnectionState::Connected;
                    self.art.clear();
                }
                Event::Disconnected(reason) => {
                    self.connection = ConnectionState::Disconnected(reason);
                    self.queues.forget_active();
                }
                Event::Status {
                    status,
                    commands_processed,
                } => {
                    if self.is_up_to_date(commands_processed) {
                        self.queues.sync_with_mpd_status(&status);
                    }
                    self.status = status;
                }
                Event::Queue {
                    uris,
                    commands_processed,
                } => {
                    if self.is_up_to_date(commands_processed) {
                        self.queues.sync_with_mpd_queue(uris);
                    }
                }
                Event::Library(songs) => {
                    self.library = Library::new(songs);
                    self.browser_width = None;
                }
                Event::ReplayGain(mode) => self.replay_gain = mode,
                Event::Art { key, image } => self.art.insert(ctx, key, image),
                Event::Error(message) => self.error = Some(message),
            }
        }
    }

    /// A snapshot taken before MPD has processed our latest commands would undo local edits.
    fn is_up_to_date(&self, commands_processed: u64) -> bool {
        commands_processed >= self.mpd.commands_sent()
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let text_field_focused = ctx.memory(|memory| memory.focused().is_some());

        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            self.focus_filter = true;
        }
        if text_field_focused || self.dialog.is_some() {
            return;
        }

        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            actions.push(Action::TogglePlayback);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
            && let View::Queue(tab) = self.view
        {
            actions.push(Action::RemoveFromTab {
                tab,
                positions: self.selection.positions(),
            });
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
            self.select_all_tracks = true;
        }
    }

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Mpd(command) => self.mpd.send(command),
            Action::TogglePlayback => {
                if self.status.state == PlayState::Playing {
                    self.mpd.send(Command::Pause);
                } else if let Some(tab) = self.queues.active() {
                    self.resume_tab(tab);
                } else if let View::Queue(tab) = self.view {
                    self.resume_tab(tab);
                }
            }
            Action::ShowView(view) => self.show_view(view),
            Action::SetBrowseMode(mode) => {
                if self.browse_mode != mode {
                    self.browse_mode = mode;
                    self.browser_search.text.clear();
                    self.browser_width = None;
                }
                self.browser_search.focus = true;
            }
            Action::SelectCollection(collection) => {
                if self.collection != collection {
                    self.selection.clear();
                }
                self.collection = collection;
                self.show_view(View::Library);
            }
            Action::PlayCollection(collection) => {
                let uris = self.library.uris_in(&collection);
                self.play_now(uris);
            }
            Action::AddCollectionToQueue { tab, collection } => {
                let uris = self.library.uris_in(&collection);
                let commands = self.queues.append(tab, uris);
                self.send_all(commands);
            }
            Action::AskNewQueueFromCollection(collection) => {
                let name = self.unique_queue_name(collection.queue_name());
                let tracks = self.library.uris_in(&collection);
                self.dialog = Some(Dialog::NewQueue { name, tracks });
            }
            Action::SortBy(column) => self.sort_by(column),
            Action::PlayNow(uris) => self.play_now(uris),
            Action::AddToQueue { tab, uris } => {
                let commands = self.queues.append(tab, uris);
                self.send_all(commands);
            }
            Action::AskNewQueue { name_hint, tracks } => {
                let name = self.unique_queue_name(&name_hint);
                self.dialog = Some(Dialog::NewQueue { name, tracks });
            }
            Action::PlayInTab { tab, position } => self.play_in_tab(tab, position),
            Action::ResumeTab(tab) => self.resume_tab(tab),
            Action::RemoveFromTab { tab, positions } => {
                let commands = self.queues.remove(tab, &positions);
                self.send_all(commands);
                self.selection.clear();
            }
            Action::MoveInTab { tab, from, to } => {
                let commands = self.queues.move_track(tab, from, to);
                self.send_all(commands);
                self.selection.select_only(to);
            }
            Action::CloseTab(tab) => self.close_tab(tab),
            Action::AskRenameTab(tab) => {
                let name = self.queues.tab(tab).queue.name.clone();
                self.dialog = Some(Dialog::RenameQueue { tab, name });
            }
            Action::OpenSavedQueue(name) => self.open_saved_queue(&name),
            Action::AskDeleteSavedQueue(name) => self.dialog = Some(Dialog::ConfirmDelete { name }),
            Action::ScanReplayGain(uris) => self.scan_replay_gain(ctx, uris),
            Action::ScanCollectionReplayGain(collection) => {
                let uris = self.library.uris_in(&collection);
                self.scan_replay_gain(ctx, uris);
            }
            Action::SaveSettings(settings) => self.save_settings(settings),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn send_all(&mut self, commands: Vec<Command>) {
        for command in commands {
            self.mpd.send(command);
        }
    }

    fn show_view(&mut self, view: View) {
        if self.view != view {
            self.selection.clear();
            if view == View::Settings {
                self.settings_draft = self.settings.clone();
            }
        }
        self.view = view;
    }

    fn sort_by(&mut self, column: SortColumn) {
        match self.view {
            View::Library => self.library_sort = SongSort::after_click(self.library_sort, column),
            View::Queue(tab) => {
                let sort = SongSort::after_click(self.queues.tab(tab).sort, column);
                self.queues.set_sort(tab, sort);
            }
            View::Settings => {}
        }
    }

    /// A queue's sort is only shown until the queue is played; then its tracks take that order.
    /// Returns the old positions in their new order, if the tab had a sort.
    fn commit_queue_sort(&mut self, tab: usize) -> Option<Vec<usize>> {
        let queue_tab = self.queues.tab(tab);
        let sort = queue_tab.sort?;

        let tracks = &queue_tab.queue.tracks;
        let mut order: Vec<usize> = (0..tracks.len()).collect();
        order.sort_by(|&a, &b| {
            let song_a = self.library.song_or_placeholder(&tracks[a]);
            let song_b = self.library.song_or_placeholder(&tracks[b]);
            sort.compare(&song_a, &song_b)
        });

        self.queues.reorder(tab, &order);
        Some(order)
    }

    fn play_in_tab(&mut self, tab: usize, position: usize) {
        let mut position = position;
        if let Some(order) = self.commit_queue_sort(tab)
            && let Some(new_position) = queues::position_after_reorder(&order, position)
        {
            position = new_position;
        }
        let commands = self.queues.play(tab, position);
        self.send_all(commands);
    }

    fn resume_tab(&mut self, tab: usize) {
        self.commit_queue_sort(tab);
        let commands = self.queues.resume(tab);
        self.send_all(commands);
    }

    /// Appends to the playing queue (or a fresh one) and starts playing the first new track.
    fn play_now(&mut self, uris: Vec<String>) {
        if uris.is_empty() {
            return;
        }

        let tab = match self.queues.active() {
            Some(tab) => tab,
            None => {
                let name = self.unique_queue_name("Queue");
                self.queues.create(&name, Vec::new())
            }
        };
        self.commit_queue_sort(tab);

        let first_new_position = self.queues.tab(tab).queue.tracks.len();
        let append_commands = self.queues.append(tab, uris);
        let play_commands = self.queues.play(tab, first_new_position);
        self.send_all(append_commands);
        self.send_all(play_commands);
    }

    fn open_saved_queue(&mut self, name: &str) {
        if let Some(tab) = self.queues.find(name) {
            self.show_view(View::Queue(tab));
            return;
        }
        match self.store.load(name) {
            Ok(queue) => {
                let tab = self.queues.open_saved(queue);
                self.show_view(View::Queue(tab));
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn close_tab(&mut self, tab: usize) {
        self.save_queues(false);
        self.queues.close(tab);

        self.view = match self.view {
            View::Queue(shown) if shown == tab => View::Library,
            View::Queue(shown) if shown > tab => View::Queue(shown - 1),
            other => other,
        };
        self.selection.clear();
    }

    fn create_queue(&mut self, name: &str, tracks: Vec<String>) {
        let tab = self.queues.create(name, tracks);
        self.save_queues(false);
        self.show_view(View::Queue(tab));
    }

    fn rename_queue(&mut self, tab: usize, new_name: &str) {
        let old = self.queues.tab(tab);
        let old_name = old.queue.name.clone();
        let had_file = old.has_file;

        self.queues.rename(tab, new_name);
        self.save_queues(false);

        if had_file
            && self.store.exists(&old_name)
            && let Err(error) = self.store.delete(&old_name)
        {
            self.error = Some(format!("{error:#}"));
        }
    }

    fn delete_saved_queue(&mut self, name: &str) {
        if let Some(tab) = self.queues.find(name) {
            self.queues.close(tab);
            self.view = View::Library;
        }
        if let Err(error) = self.store.delete(name) {
            self.error = Some(format!("{error:#}"));
        }
    }

    fn is_queue_name_available(&self, name: &str) -> bool {
        self.queues.find(name).is_none() && !self.store.exists(name)
    }

    fn unique_queue_name(&self, base: &str) -> String {
        let base = if base.is_empty() { "Queue" } else { base };
        if self.is_queue_name_available(base) {
            return base.to_string();
        }
        let mut number = 2;
        loop {
            let candidate = format!("{base} {number}");
            if self.is_queue_name_available(&candidate) {
                return candidate;
            }
            number += 1;
        }
    }

    fn save_queues(&mut self, include_unchanged: bool) {
        if let Err(error) = self.queues.save(&self.store, include_unchanged) {
            self.error = Some(format!("{error:#}"));
        }
        self.autosave_due = None;
    }

    /// The settings take effect even if they could not be written to disk.
    fn save_settings(&mut self, settings: Settings) {
        if let Err(error) = self.settings_store.save(&settings) {
            self.error = Some(format!("{error:#}"));
        }
        self.settings_draft = settings.clone();
        self.settings = settings;
    }

    fn scan_replay_gain(&mut self, ctx: &egui::Context, uris: Vec<String>) {
        if self.replay_gain_scan.is_some() {
            self.error = Some("A ReplayGain scan is already running".to_string());
            return;
        }

        let music_dir = match &self.settings.music_dir {
            Some(dir) if dir.is_dir() => dir.clone(),
            other => {
                let path = match other {
                    Some(dir) => dir.display().to_string(),
                    None => String::new(),
                };
                self.dialog = Some(Dialog::MusicFolder {
                    path,
                    pending_scan: uris,
                });
                return;
            }
        };

        let mut songs = Vec::new();
        for uri in &uris {
            if let Some(song) = self.library.find(uri) {
                songs.push(song);
            }
        }
        let groups = replaygain::group_by_album(&songs);
        if groups.is_empty() {
            return;
        }
        self.replay_gain_scan = Some(ReplayGainScan::start(groups, music_dir, ctx.clone()));
    }

    /// Once the scan is done, MPD re-reads the tagged files so the new gains apply.
    fn finish_replay_gain_scan(&mut self) {
        let Some(scan) = &mut self.replay_gain_scan else {
            return;
        };
        let Some(result) = scan.poll() else {
            return;
        };
        self.replay_gain_scan = None;

        if !result.directories.is_empty() {
            self.mpd.send(Command::UpdateDatabase(result.directories));
        }
        let error_count = result.errors.len();
        if let Some(first_error) = result.errors.into_iter().next() {
            self.error = Some(if error_count > 1 {
                format!("{first_error} (and {} more errors)", error_count - 1)
            } else {
                first_error
            });
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        let session = self.queues.session();
        if session != self.saved_session {
            if let Err(error) = self.store.save_session(&session) {
                self.error = Some(format!("{error:#}"));
            }
            self.saved_session = session;
        }

        if !self.queues.has_unsaved_changes() {
            return;
        }
        let now = Instant::now();
        let due = *self.autosave_due.get_or_insert(now + AUTOSAVE_DELAY);
        if now >= due {
            self.save_queues(false);
        } else {
            ctx.request_repaint_after(due - now);
        }
    }

    fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };

        let outcome = dialogs::show(ctx, &mut dialog, |name| self.is_queue_name_available(name));

        match outcome {
            DialogOutcome::StillOpen => self.dialog = Some(dialog),
            DialogOutcome::Cancelled => {}
            DialogOutcome::Confirmed => match dialog {
                Dialog::NewQueue { name, tracks } => self.create_queue(name.trim(), tracks),
                Dialog::RenameQueue { tab, name } => self.rename_queue(tab, name.trim()),
                Dialog::ConfirmDelete { name } => self.delete_saved_queue(&name),
                Dialog::MusicFolder { path, pending_scan } => {
                    let mut settings = self.settings.clone();
                    settings.music_dir = Some(PathBuf::from(path.trim()));
                    self.save_settings(settings);
                    self.scan_replay_gain(ctx, pending_scan);
                }
            },
        }
    }

    fn queue_names(&self) -> Vec<String> {
        self.queues
            .tabs()
            .iter()
            .map(|tab| tab.queue.name.clone())
            .collect()
    }

    fn now_playing(&self) -> Option<Song> {
        let tab = self.queues.active()?;
        let position = self.status.position?;
        let uri = self.queues.tab(tab).queue.tracks.get(position)?;
        Some(self.library.song_or_placeholder(uri).into_owned())
    }

    fn show_status_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            match &self.connection {
                ConnectionState::Connecting => {
                    ui.weak("Connecting to MPD…");
                }
                ConnectionState::Connected => {
                    ui.weak(format!("{} songs in library", self.library.len()));
                    if self.status.updating_database {
                        ui.spinner();
                        ui.weak("Rescanning…");
                    } else if ui
                        .small_button("⟳")
                        .on_hover_text("Rescan library")
                        .clicked()
                    {
                        actions.push(Action::Mpd(Command::UpdateDatabase(vec![String::new()])));
                    }
                }
                ConnectionState::Disconnected(reason) => {
                    ui.weak(format!("Disconnected: {reason}"));
                }
            }

            if let Some(scan) = &self.replay_gain_scan {
                let (finished, total) = scan.progress();
                ui.separator();
                ui.spinner();
                ui.weak(format!("ReplayGain: {finished} of {total} scanned…"));
            }

            if let Some(error) = &self.error {
                ui.separator();
                ui.colored_label(ui.visuals().error_fg_color, error);
                if ui.small_button("×").clicked() {
                    self.error = None;
                }
            }
        });
    }

    fn show_filter(&mut self, ui: &mut egui::Ui) {
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .id(egui::Id::unique(FILTER_ID))
                .hint_text("Filter: words, artist:, album:, title:, genre:, date:, path:, -exclude  (Ctrl+F)")
                .desired_width(f32::INFINITY),
        );

        if self.focus_filter {
            response.request_focus();
            self.focus_filter = false;
        }
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.filter.clear();
        }
        if response.changed() {
            self.selection.clear();
        }
    }

    fn show_library_view(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if self.library.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.weak("The library is empty, or MPD is not connected yet.");
            });
            return;
        }

        let indices = self.library.songs_in(&self.collection);
        let query = Query::parse(&self.filter);

        let mut rows = Vec::new();
        for index in indices {
            let song = self.library.song(index);
            if query.matches(song) {
                rows.push(TrackRow {
                    position: index,
                    song: Cow::Borrowed(song),
                });
            }
        }
        if let Some(sort) = self.library_sort {
            tracks::sort_rows(&mut rows, sort);
        }

        if self.select_all_tracks {
            self.selection.select_all(&row_positions(&rows));
        }

        ui.horizontal(|ui| {
            if let Some(first) = rows.first() {
                nowplaying::album_art(ui, &first.song, 48.0, &mut self.art, &mut self.mpd);
            }
            ui.vertical(|ui| {
                ui.strong(self.collection.title());
                ui.weak(format!("{} tracks", rows.len()));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open as queue…").clicked() {
                    actions.push(Action::AskNewQueue {
                        name_hint: self.collection.queue_name().to_string(),
                        tracks: row_uris(&rows),
                    });
                }
                if ui.button("▶ Play").clicked() {
                    actions.push(Action::PlayNow(row_uris(&rows)));
                }
            });
        });
        ui.separator();

        let queue_names = self.queue_names();
        let table = TrackTable {
            rows: &rows,
            source: TrackSource::Library,
            playing_position: None,
            queue_names: &queue_names,
            allow_reorder: false,
            sort: self.library_sort,
        };
        tracks::show(
            ui,
            table,
            &mut self.column_widths,
            &mut self.selection,
            actions,
        );
    }

    fn show_queue_view(&mut self, ui: &mut egui::Ui, tab: usize, actions: &mut Vec<Action>) {
        let is_active = self.queues.active() == Some(tab);
        let queue = &self.queues.tab(tab).queue;
        let sort = self.queues.tab(tab).sort;
        let query = Query::parse(&self.filter);

        let mut rows = Vec::new();
        let mut total = Duration::ZERO;
        for (position, uri) in queue.tracks.iter().enumerate() {
            let song = self.library.song_or_placeholder(uri);
            total += song.duration.unwrap_or_default();
            if query.matches(&song) {
                rows.push(TrackRow { position, song });
            }
        }
        if let Some(sort) = sort {
            tracks::sort_rows(&mut rows, sort);
        }

        if self.select_all_tracks {
            self.selection.select_all(&row_positions(&rows));
        }

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.strong(&queue.name);
                ui.weak(format!(
                    "{} tracks · {}{}",
                    queue.tracks.len(),
                    crate::ui::format_duration(total),
                    if is_active { " · playing in MPD" } else { "" }
                ));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if is_active { "▶ Play" } else { "▶ Resume" };
                if ui.button(label).clicked() {
                    actions.push(Action::ResumeTab(tab));
                }
            });
        });
        ui.separator();

        let queue_names = self.queue_names();
        let playing_position = if is_active {
            self.status.position
        } else {
            queue.current
        };
        let table = TrackTable {
            rows: &rows,
            source: TrackSource::Queue { tab },
            playing_position,
            queue_names: &queue_names,
            allow_reorder: query.is_empty() && sort.is_none(),
            sort,
        };
        tracks::show(
            ui,
            table,
            &mut self.column_widths,
            &mut self.selection,
            actions,
        );
    }
}

fn row_positions(rows: &[TrackRow]) -> Vec<usize> {
    rows.iter().map(|row| row.position).collect()
}

fn row_uris(rows: &[TrackRow]) -> Vec<String> {
    rows.iter().map(|row| row.song.uri.clone()).collect()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_mpd_events(&ctx);
        self.finish_replay_gain_scan();

        let mut actions = Vec::new();
        self.handle_keyboard(&ctx, &mut actions);

        egui::Panel::top("transport").show(ui, |ui| {
            ui.add_space(4.0);
            let view = TransportView {
                status: &self.status,
                replay_gain: self.replay_gain,
                connection: &self.connection,
            };
            transport::show(ui, view, &mut self.transport, &mut actions);
            ui.add_space(4.0);
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| self.show_status_bar(ui, &mut actions));

        let panel_margin = egui::Frame::side_top_panel(ui.style())
            .total_margin()
            .sum()
            .x;

        let browser_width = *self
            .browser_width
            .get_or_insert_with(|| browser::content_width(ui, &self.library, self.browse_mode));
        let browser_min = browser::min_width(ui) + panel_margin;
        egui::Panel::left("browser")
            .resizable(true)
            .default_size(260.0)
            .size_range(browser_min..=(browser_width + panel_margin).max(browser_min))
            .show(ui, |ui| {
                let queue_names = self.queue_names();
                let view = BrowserView {
                    library: &self.library,
                    mode: self.browse_mode,
                    selected: &self.collection,
                    queue_names: &queue_names,
                };
                browser::show(ui, view, &mut self.browser_search, &mut actions);
            });

        let song = self.now_playing();
        let art_width = nowplaying::content_width(song.as_ref(), &self.art);
        egui::Panel::right("now_playing")
            .resizable(true)
            .default_size(280.0)
            .size_range(MIN_PANEL_WIDTH..=(art_width + panel_margin).max(MIN_PANEL_WIDTH))
            .show(ui, |ui| {
                nowplaying::show(ui, song.as_ref(), &mut self.art, &mut self.mpd);
            });

        egui::CentralPanel::default_margins().show(ui, |ui| {
            tabs::show(ui, &self.queues, &self.store, self.view, &mut actions);
            ui.separator();
            if self.view != View::Settings {
                self.show_filter(ui);
                ui.add_space(4.0);
            }
            match self.view {
                View::Library => self.show_library_view(ui, &mut actions),
                View::Queue(tab) => self.show_queue_view(ui, tab, &mut actions),
                View::Settings => {
                    settings::show(ui, &mut self.settings_draft, &self.settings, &mut actions)
                }
            }
        });
        self.select_all_tracks = false;

        self.show_dialog(&ctx);

        for action in actions {
            self.apply(&ctx, action);
        }
        self.autosave(&ctx);
    }

    fn on_exit(&mut self) {
        self.save_queues(true);
        let _ = self.store.save_session(&self.queues.session());
        if matches!(self.connection, ConnectionState::Connected) {
            self.mpd.stop_and_wait(EXIT_STOP_TIMEOUT);
        }
    }
}
