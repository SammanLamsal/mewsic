use eframe::egui;

use crate::app::Action;
use crate::library::{Collection, Directory, Library};
use crate::ui::text_width;

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

pub struct BrowserView<'a> {
    pub library: &'a Library,
    pub mode: BrowseMode,
    pub selected: &'a Collection,
    pub queue_names: &'a [String],
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

    let all_selected = *view.selected == Collection::All;
    let all = ui.selectable_label(all_selected, Collection::All.title());
    handle_collection_response(&all, Collection::All, &view, actions);

    let words = search_words(&search.text);
    if view.mode == BrowseMode::Folders && words.is_empty() {
        show_folders(ui, &view, actions);
    } else {
        show_list(ui, &view, &words, actions);
    }
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

fn search_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

fn matches_search(label: &str, words: &[String]) -> bool {
    let label = label.to_lowercase();
    words.iter().all(|word| label.contains(word.as_str()))
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

fn show_folders(ui: &mut egui::Ui, view: &BrowserView, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for directory in &view.library.root().children {
                show_directory(ui, directory, view, actions);
            }
        });
}

fn show_directory(
    ui: &mut egui::Ui,
    directory: &Directory,
    view: &BrowserView,
    actions: &mut Vec<Action>,
) {
    let collection = Collection::Directory(directory.path.clone());
    let is_selected = *view.selected == collection;

    if directory.children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            let response = ui.selectable_label(is_selected, &directory.name);
            handle_collection_response(&response, collection, view, actions);
        });
        return;
    }

    let id = ui.make_persistent_id(&directory.path);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            let response = ui.selectable_label(is_selected, &directory.name);
            handle_collection_response(&response, collection, view, actions);
        })
        .body(|ui| {
            for child in &directory.children {
                show_directory(ui, child, view, actions);
            }
        });
}

fn show_list(ui: &mut egui::Ui, view: &BrowserView, words: &[String], actions: &mut Vec<Action>) {
    let count = list_len(view.library, view.mode);
    let indices: Vec<usize> = if words.is_empty() {
        (0..count).collect()
    } else {
        (0..count)
            .filter(|&index| {
                let (_, label) = list_entry(view.library, view.mode, index);
                matches_search(&label, words)
            })
            .collect()
    };

    if indices.is_empty() && !words.is_empty() {
        ui.weak("No matches");
        return;
    }

    let row_height = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_height,
        indices.len(),
        |ui, rows| {
            for &index in &indices[rows] {
                let (collection, label) = list_entry(view.library, view.mode, index);
                let is_selected = *view.selected == collection;
                let response = ui.selectable_label(is_selected, label);
                handle_collection_response(&response, collection, view, actions);
            }
        },
    );
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
        actions.push(Action::PlayCollection(collection.clone()));
    }

    response.context_menu(|ui| {
        if ui.button("Play now").clicked() {
            actions.push(Action::PlayCollection(collection.clone()));
        }
        if ui.button("Open as new queue…").clicked() {
            actions.push(Action::AskNewQueueFromCollection(collection.clone()));
        }
        ui.menu_button("Add to queue", |ui| {
            if view.queue_names.is_empty() {
                ui.label("No open queues");
            }
            for (tab, name) in view.queue_names.iter().enumerate() {
                if ui.button(name).clicked() {
                    actions.push(Action::AddCollectionToQueue {
                        tab,
                        collection: collection.clone(),
                    });
                }
            }
        });
        ui.separator();
        if ui.button("Scan ReplayGain").clicked() {
            actions.push(Action::ScanCollectionReplayGain(collection.clone()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_every_word_ignoring_case() {
        let words = search_words("alpha MORN");
        assert!(matches_search("Morning — Alpha", &words));
        assert!(!matches_search("Morning — Bravo", &words));
        assert!(matches_search("anything", &search_words("  ")));
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
                };
                show(ui, view, &mut BrowserSearch::default(), &mut Vec::new());
                assert!(ui.min_rect().width() <= min_width + 0.5);
            });
        });
        output.textures_delta.clear();
    }
}
