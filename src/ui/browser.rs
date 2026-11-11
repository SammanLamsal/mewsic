use eframe::egui;
use egui::collapsing_header::CollapsingState;

use crate::app::Action;
use crate::filter::Needle;
use crate::library::{Collection, Directory, Library};
use crate::ui::motion::{self, Motion};
use crate::ui::{replay_gain_menu, text_width};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrowseMode {
    Folders,
    Artists,
    Albums,
    Genres,
}

const MODES: [(BrowseMode, &str); 4] = [
    (BrowseMode::Folders, "Folders"),
    (BrowseMode::Artists, "Artists"),
    (BrowseMode::Albums, "Albums"),
    (BrowseMode::Genres, "Genres"),
];

impl BrowseMode {
    pub fn next(self) -> BrowseMode {
        self.step(1)
    }

    pub fn previous(self) -> BrowseMode {
        self.step(MODES.len() - 1)
    }

    /// The mode `steps` places further along the mode tabs, wrapping around.
    fn step(self, steps: usize) -> BrowseMode {
        let index = MODES
            .iter()
            .position(|&(mode, _)| mode == self)
            .unwrap_or(0);
        MODES[(index + steps) % MODES.len()].0
    }
}

/// A keyboard command for the sidebar, applied by `show` since it knows which entries are visible.
/// The selected collection doubles as the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrowserCommand {
    Move(Motion),
    Collapse,
    Expand,
    Play,
}

pub struct BrowserView<'a> {
    pub library: &'a Library,
    pub mode: BrowseMode,
    pub selected: &'a Collection,
    pub queue_names: &'a [String],
    pub command: Option<BrowserCommand>,
}

#[derive(Default)]
pub struct BrowserSearch {
    pub text: String,
    pub focus: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    view: BrowserView,
    search: &mut BrowserSearch,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        for (mode, label) in MODES {
            if ui.selectable_label(view.mode == mode, label).clicked() {
                actions.push(Action::SetBrowseMode(mode));
            }
        }
    });
    show_search(ui, view.mode, search);
    ui.separator();

    let words = search_words(&search.text);
    let scroll_target = match view.command {
        Some(command) => apply_command(ui, command, &view, &words, actions),
        None => None,
    };

    let all_selected = *view.selected == Collection::All;
    let all = ui.selectable_label(all_selected, Collection::All.title());
    handle_collection_response(&all, Collection::All, &view, actions);

    if shows_tree(view.mode, &words) {
        show_folders(ui, &view, scroll_target.as_ref(), actions);
    } else {
        show_list(ui, &view, &words, scroll_target.as_ref(), actions);
    }
}

fn shows_tree(mode: BrowseMode, words: &[Needle]) -> bool {
    mode == BrowseMode::Folders && words.is_empty()
}

/// Returns the entry to scroll to, if the cursor moved.
fn apply_command(
    ui: &egui::Ui,
    command: BrowserCommand,
    view: &BrowserView,
    words: &[Needle],
    actions: &mut Vec<Action>,
) -> Option<Collection> {
    let ctx = ui.ctx();
    match command {
        BrowserCommand::Move(motion) => {
            let entries = visible_entries(ctx, view, words);
            let cursor_row = entries.iter().position(|entry| entry == view.selected);
            let half_page = motion::half_page(ui, ui.spacing().interact_size.y);
            let target = motion::target_row(motion, cursor_row, entries.len(), half_page)?;
            select(entries[target].clone(), actions)
        }
        BrowserCommand::Expand => {
            if shows_tree(view.mode, words) {
                expand_folder(ctx, view);
            }
            None
        }
        BrowserCommand::Collapse => {
            if !shows_tree(view.mode, words) {
                return None;
            }
            collapse_folder(ctx, view, actions)
        }
        BrowserCommand::Play => {
            actions.push(Action::PlayNow(view.library.uris_in(view.selected)));
            None
        }
    }
}

fn select(collection: Collection, actions: &mut Vec<Action>) -> Option<Collection> {
    actions.push(Action::SelectCollection(collection.clone()));
    Some(collection)
}

/// Every entry the cursor can reach, in the order shown, starting with "All".
fn visible_entries(ctx: &egui::Context, view: &BrowserView, words: &[Needle]) -> Vec<Collection> {
    let mut entries = vec![Collection::All];
    if shows_tree(view.mode, words) {
        for directory in &view.library.root().children {
            push_visible_folders(ctx, directory, &mut entries);
        }
    } else {
        for index in filtered_indices(view, words) {
            let (collection, _) = list_entry(view.library, view.mode, index);
            entries.push(collection);
        }
    }
    entries
}

fn push_visible_folders(ctx: &egui::Context, directory: &Directory, entries: &mut Vec<Collection>) {
    entries.push(Collection::Directory(directory.path.clone()));
    if !is_folder_open(ctx, &directory.path) {
        return;
    }
    for child in &directory.children {
        push_visible_folders(ctx, child, entries);
    }
}

fn expand_folder(ctx: &egui::Context, view: &BrowserView) {
    let Collection::Directory(path) = view.selected else {
        return;
    };
    let Some(directory) = find_directory(view.library.root(), path) else {
        return;
    };
    if !directory.children.is_empty() {
        set_folder_open(ctx, path, true);
    }
}

/// Closes the selected folder, or moves to its parent if it is already closed.
fn collapse_folder(
    ctx: &egui::Context,
    view: &BrowserView,
    actions: &mut Vec<Action>,
) -> Option<Collection> {
    let Collection::Directory(path) = view.selected else {
        return None;
    };
    if is_folder_open(ctx, path) {
        set_folder_open(ctx, path, false);
        return None;
    }
    let parent = parent_path(path)?;
    select(Collection::Directory(parent.to_string()), actions)
}

fn parent_path(path: &str) -> Option<&str> {
    let (parent, _) = path.rsplit_once('/')?;
    Some(parent)
}

fn find_directory<'a>(directory: &'a Directory, path: &str) -> Option<&'a Directory> {
    for child in &directory.children {
        if child.path == path {
            return Some(child);
        }
        let child_prefix = format!("{}/", child.path);
        if path.starts_with(&child_prefix) {
            return find_directory(child, path);
        }
    }
    None
}

/// Keyed by path alone, so the keyboard can open and close folders from outside the tree's `Ui`.
fn folder_state(ctx: &egui::Context, path: &str) -> CollapsingState {
    let id = egui::Id::unique(("browser_folder", path));
    CollapsingState::load_with_default_open(ctx, id, false)
}

fn is_folder_open(ctx: &egui::Context, path: &str) -> bool {
    folder_state(ctx, path).is_open()
}

fn set_folder_open(ctx: &egui::Context, path: &str, open: bool) {
    let mut state = folder_state(ctx, path);
    state.set_open(open);
    state.store(ctx);
}

fn show_search(ui: &mut egui::Ui, mode: BrowseMode, search: &mut BrowserSearch) {
    let hint = match mode {
        BrowseMode::Folders => "Search folders",
        BrowseMode::Artists => "Search artists",
        BrowseMode::Albums => "Search albums",
        BrowseMode::Genres => "Search genres",
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut search.text)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );

    if search.focus {
        response.request_focus();
        search.focus = false;
    }
    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        search.text.clear();
    }
}

fn search_words(text: &str) -> Vec<Needle> {
    text.split_whitespace().map(Needle::new).collect()
}

fn matches_search(label: &str, words: &[Needle]) -> bool {
    words.iter().all(|word| word.is_in(label))
}

/// The width the panel needs to show its widest entry without clipping.
pub fn content_width(ui: &egui::Ui, library: &Library, mode: BrowseMode) -> f32 {
    let button_padding = 2.0 * ui.spacing().button_padding.x;
    let item_spacing = ui.spacing().item_spacing.x;

    let mut widest_entry = text_width(ui, Collection::All.title());
    match mode {
        BrowseMode::Folders => {
            let row_prefix = ui.spacing().indent + item_spacing;
            for directory in &library.root().children {
                widest_entry = widest_entry.max(row_prefix + directory_width(ui, directory));
            }
        }
        mode => {
            for index in 0..list_len(library, mode) {
                let (_, label) = list_entry(library, mode, index);
                widest_entry = widest_entry.max(text_width(ui, &label));
            }
        }
    }

    let scroll_bar = ui.spacing().scroll.bar_width;
    widest_entry + button_padding + scroll_bar
}

/// The mode tabs sit in one row that can't wrap, so the panel must never be narrower than them.
pub fn min_width(ui: &egui::Ui) -> f32 {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let button_padding = 2.0 * ui.spacing().button_padding.x;
    let gaps = ui.spacing().item_spacing.x * (MODES.len() - 1) as f32;

    let mut width = gaps;
    for (_, label) in MODES {
        let galley = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                label.to_string(),
                font_id.clone(),
                egui::Color32::PLACEHOLDER,
            )
        });
        width += galley.size().x + button_padding;
    }
    width
}

/// Width of the widest row in this directory's subtree, measured from its own indent level.
fn directory_width(ui: &egui::Ui, directory: &Directory) -> f32 {
    let mut widest = text_width(ui, &directory.name);
    for child in &directory.children {
        widest = widest.max(ui.spacing().indent + directory_width(ui, child));
    }
    widest
}

fn show_folders(
    ui: &mut egui::Ui,
    view: &BrowserView,
    scroll_target: Option<&Collection>,
    actions: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for directory in &view.library.root().children {
                show_directory(ui, directory, view, scroll_target, actions);
            }
        });
}

fn show_directory(
    ui: &mut egui::Ui,
    directory: &Directory,
    view: &BrowserView,
    scroll_target: Option<&Collection>,
    actions: &mut Vec<Action>,
) {
    let collection = Collection::Directory(directory.path.clone());
    let is_selected = *view.selected == collection;
    let is_scroll_target = scroll_target == Some(&collection);

    if directory.children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            let response = ui.selectable_label(is_selected, &directory.name);
            if is_scroll_target {
                response.scroll_to_me(None);
            }
            handle_collection_response(&response, collection, view, actions);
        });
        return;
    }

    folder_state(ui.ctx(), &directory.path)
        .show_header(ui, |ui| {
            let response = ui.selectable_label(is_selected, &directory.name);
            if is_scroll_target {
                response.scroll_to_me(None);
            }
            handle_collection_response(&response, collection, view, actions);
        })
        .body(|ui| {
            for child in &directory.children {
                show_directory(ui, child, view, scroll_target, actions);
            }
        });
}

fn filtered_indices(view: &BrowserView, words: &[Needle]) -> Vec<usize> {
    let count = list_len(view.library, view.mode);
    if words.is_empty() {
        return (0..count).collect();
    }
    (0..count)
        .filter(|&index| {
            let (_, label) = list_entry(view.library, view.mode, index);
            matches_search(&label, words)
        })
        .collect()
}

fn show_list(
    ui: &mut egui::Ui,
    view: &BrowserView,
    words: &[Needle],
    scroll_target: Option<&Collection>,
    actions: &mut Vec<Action>,
) {
    let indices = filtered_indices(view, words);

    if indices.is_empty() && !words.is_empty() {
        ui.weak("No matches");
        return;
    }

    let mut scroll_row = None;
    if let Some(target) = scroll_target {
        scroll_row = indices
            .iter()
            .position(|&index| list_entry(view.library, view.mode, index).0 == *target);
    }

    let row_height = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_height,
        indices.len(),
        |ui, rows| {
            if let Some(row) = scroll_row {
                scroll_to_list_row(ui, row, rows.start, row_height);
            }
            for &index in &indices[rows] {
                let (collection, label) = list_entry(view.library, view.mode, index);
                let is_selected = *view.selected == collection;
                let response = ui.selectable_label(is_selected, label);
                handle_collection_response(&response, collection, view, actions);
            }
        },
    );
}

/// `show_rows` only lays out the rows in view, so the target's rect is worked out from
/// the first row laid out, which sits at the top of the closure's `ui`.
fn scroll_to_list_row(ui: &egui::Ui, row: usize, first_row: usize, row_height: f32) {
    let row_pitch = row_height + ui.spacing().item_spacing.y;
    let top = ui.max_rect().top() + (row as f32 - first_row as f32) * row_pitch;
    let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + row_height);
    ui.scroll_to_rect(rect, None);
}

fn list_len(library: &Library, mode: BrowseMode) -> usize {
    match mode {
        BrowseMode::Folders => library.directories().len(),
        BrowseMode::Artists => library.artists().len(),
        BrowseMode::Albums => library.albums().len(),
        BrowseMode::Genres => library.genres().len(),
    }
}

fn list_entry(library: &Library, mode: BrowseMode, index: usize) -> (Collection, String) {
    let collection = match mode {
        BrowseMode::Folders => Collection::Directory(library.directories()[index].clone()),
        BrowseMode::Artists => Collection::Artist(library.artists()[index].clone()),
        BrowseMode::Albums => Collection::Album(library.albums()[index].clone()),
        BrowseMode::Genres => Collection::Genre(library.genres()[index].clone()),
    };
    let label = match &collection {
        Collection::Album(album) => format!("{} — {}", album.title(), album.artist_title()),
        other => other.title().to_string(),
    };
    (collection, label)
}

fn handle_collection_response(
    response: &egui::Response,
    collection: Collection,
    view: &BrowserView,
    actions: &mut Vec<Action>,
) {
    if response.clicked() {
        actions.push(Action::SelectCollection(collection.clone()));
    }
    if response.double_clicked() {
        actions.push(Action::PlayNow(view.library.uris_in(&collection)));
    }

    response.context_menu(|ui| {
        if ui.button("Play now").clicked() {
            actions.push(Action::PlayNow(view.library.uris_in(&collection)));
        }
        if ui.button("Open as new queue…").clicked() {
            actions.push(Action::AskNewQueue {
                name_hint: collection.queue_name().to_string(),
                tracks: view.library.uris_in(&collection),
            });
        }
        ui.menu_button("Add to queue", |ui| {
            if view.queue_names.is_empty() {
                ui.label("No open queues");
            }
            for (tab, name) in view.queue_names.iter().enumerate() {
                if ui.button(name).clicked() {
                    actions.push(Action::AddToQueue {
                        tab,
                        uris: view.library.uris_in(&collection),
                    });
                }
            }
        });
        ui.separator();
        replay_gain_menu(ui, || view.library.uris_in(&collection), actions);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Song;

    #[test]
    fn search_matches_every_word_ignoring_case() {
        let words = search_words("alpha MORN");
        assert!(matches_search("Morning — Alpha", &words));
        assert!(!matches_search("Morning — Bravo", &words));
        assert!(matches_search("anything", &search_words("  ")));
    }

    #[test]
    fn search_ignores_punctuation_and_accents() {
        assert!(matches_search("Don’t Stop — Alpha", &search_words("dont")));
        assert!(matches_search("Beyoncé", &search_words("beyonce")));
    }

    fn folder(path: &str) -> Collection {
        Collection::Directory(path.to_string())
    }

    #[test]
    fn the_cursor_only_reaches_folders_inside_open_ones() {
        let library = Library::new(vec![
            Song::from_uri("Music/Jazz/a.flac"),
            Song::from_uri("Music/Blues/b.flac"),
        ]);
        let view = BrowserView {
            library: &library,
            mode: BrowseMode::Folders,
            selected: &Collection::All,
            queue_names: &[],
            command: None,
        };
        let ctx = egui::Context::default();

        let entries = visible_entries(&ctx, &view, &[]);
        assert_eq!(entries, [Collection::All, folder("Music")]);

        set_folder_open(&ctx, "Music", true);
        let entries = visible_entries(&ctx, &view, &[]);
        assert_eq!(
            entries,
            [
                Collection::All,
                folder("Music"),
                folder("Music/Blues"),
                folder("Music/Jazz")
            ]
        );
    }

    #[test]
    fn finds_nested_folders_and_their_parents() {
        let library = Library::new(vec![Song::from_uri("Music/Jazz/Live/a.flac")]);
        let live = find_directory(library.root(), "Music/Jazz/Live");
        assert_eq!(live.map(|directory| directory.name.as_str()), Some("Live"));
        assert_eq!(parent_path("Music/Jazz/Live"), Some("Music/Jazz"));
        assert_eq!(parent_path("Music"), None);
    }

    #[test]
    fn browse_modes_wrap_around() {
        assert_eq!(BrowseMode::Genres.next(), BrowseMode::Folders);
        assert_eq!(BrowseMode::Folders.previous(), BrowseMode::Genres);
        assert_eq!(BrowseMode::Artists.next(), BrowseMode::Albums);
    }

    #[test]
    fn nothing_spills_past_the_minimum_width() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let min_width = min_width(ui);
            let builder = egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                ui.max_rect().min,
                egui::vec2(min_width, 400.0),
            ));
            ui.scope_builder(builder, |ui| {
                let library = Library::default();
                let view = BrowserView {
                    library: &library,
                    mode: BrowseMode::Folders,
                    selected: &Collection::All,
                    queue_names: &[],
                    command: None,
                };
                show(ui, view, &mut BrowserSearch::default(), &mut Vec::new());
                assert!(ui.min_rect().width() <= min_width + 0.5);
            });
        });
        output.textures_delta.clear();
    }
}
