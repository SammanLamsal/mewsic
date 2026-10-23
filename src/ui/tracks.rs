use std::borrow::Cow;
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use crate::app::Action;
use crate::library::{Song, SongSort, SortColumn};
use crate::ui::{format_duration, text_width};

const ROW_HEIGHT: f32 = 22.0;
const MIN_TITLE_WIDTH: f32 = 80.0;
const MIN_ARTIST_WIDTH: f32 = 60.0;
const ASCENDING_ARROW: &str = " ⏶";
const DESCENDING_ARROW: &str = " ⏷";
const SORT_COLUMNS: [SortColumn; 4] = [
    SortColumn::Title,
    SortColumn::Artist,
    SortColumn::Album,
    SortColumn::Duration,
];

pub struct TrackRow<'a> {
    /// Index into the library for library rows, or the queue position for queue rows.
    pub position: usize,
    pub song: Cow<'a, Song>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum TrackSource {
    Library,
    Queue { tab: usize },
}

pub struct TrackTable<'a> {
    pub rows: &'a [TrackRow<'a>],
    pub source: TrackSource,
    pub playing_position: Option<usize>,
    pub queue_names: &'a [String],
    pub allow_reorder: bool,
    pub sort: Option<SongSort>,
}

#[derive(Clone, Copy)]
struct DraggedTrack(usize);

/// How far the Title and Artist columns may be widened: up to their longest entry.
/// Measured again only when the rows change.
#[derive(Default)]
pub struct ColumnWidths {
    rows_hash: Option<u64>,
    title: f32,
    artist: f32,
}

impl ColumnWidths {
    fn update(&mut self, ui: &egui::Ui, rows: &[TrackRow]) {
        let mut hasher = DefaultHasher::new();
        for row in rows {
            row.song.uri.hash(&mut hasher);
        }
        let rows_hash = hasher.finish();
        if self.rows_hash == Some(rows_hash) {
            return;
        }

        let arrow = text_width(ui, ASCENDING_ARROW).max(text_width(ui, DESCENDING_ARROW));
        let mut title = text_width(ui, header_title(SortColumn::Title)) + arrow;
        let mut artist = text_width(ui, header_title(SortColumn::Artist)) + arrow;
        for row in rows {
            title = title.max(text_width(ui, row.song.display_title()));
            artist = artist.max(text_width(ui, &row.song.artist));
        }

        let padding = ui.spacing().item_spacing.x;
        self.title = (title + padding).max(MIN_TITLE_WIDTH);
        self.artist = (artist + padding).max(MIN_ARTIST_WIDTH);
        self.rows_hash = Some(rows_hash);
    }
}

pub fn show(
    ui: &mut egui::Ui,
    table: TrackTable,
    widths: &mut ColumnWidths,
    selection: &mut Selection,
    actions: &mut Vec<Action>,
) {
    let modifiers = ui.input(|input| input.modifiers);
    let ctx = ui.ctx().clone();
    widths.update(ui, table.rows);

    let sense = if table.allow_reorder {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };

    TableBuilder::new(ui)
        .striped(true)
        .sense(sense)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(36.0))
        .column(
            Column::initial(300.0)
                .range(MIN_TITLE_WIDTH..=widths.title)
                .resizable(true)
                .clip(true),
        )
        .column(
            Column::initial(200.0)
                .range(MIN_ARTIST_WIDTH..=widths.artist)
                .resizable(true)
                .clip(true),
        )
        .column(Column::remainder().at_least(60.0).clip(true))
        .column(Column::exact(56.0))
        .header(ROW_HEIGHT, |mut header| {
            header.col(|ui| {
                ui.strong("#");
            });
            for column in SORT_COLUMNS {
                header.col(|ui| sort_header(ui, column, table.sort, actions));
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, table.rows.len(), |mut row| {
                let track = &table.rows[row.index()];
                row.set_selected(selection.contains(track.position));

                row.col(|ui| {
                    cell(ui, &number_label(&table, track));
                });
                row.col(|ui| {
                    cell(ui, track.song.display_title());
                });
                row.col(|ui| {
                    cell(ui, &track.song.artist);
                });
                row.col(|ui| {
                    cell(ui, &track.song.album);
                });
                row.col(|ui| {
                    let duration = track.song.duration.map(format_duration).unwrap_or_default();
                    cell(ui, &duration);
                });

                let response = row.response();
                handle_row_clicks(&response, &table, track, modifiers, selection, actions);
                if table.allow_reorder {
                    handle_row_drag(&ctx, &response, &table, track, actions);
                }
                response.context_menu(|ui| show_context_menu(ui, &table, selection, actions));
            });
        });
}

pub fn sort_rows(rows: &mut [TrackRow], sort: SongSort) {
    rows.sort_by(|a, b| sort.compare(&a.song, &b.song));
}

fn header_title(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Title => "Title",
        SortColumn::Artist => "Artist",
        SortColumn::Album => "Album",
        SortColumn::Duration => "Time",
    }
}

fn sort_header(
    ui: &mut egui::Ui,
    column: SortColumn,
    sort: Option<SongSort>,
    actions: &mut Vec<Action>,
) {
    let mut text = header_title(column).to_string();
    if let Some(sort) = sort
        && sort.column == column
    {
        text.push_str(if sort.descending {
            DESCENDING_ARROW
        } else {
            ASCENDING_ARROW
        });
    }

    let label = egui::Label::new(egui::RichText::new(text).strong())
        .sense(egui::Sense::click())
        .selectable(false);
    let response = ui
        .add(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        actions.push(Action::SortBy(column));
    }
}

fn cell(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).truncate().selectable(false));
}

fn number_label(table: &TrackTable, track: &TrackRow) -> String {
    if table.playing_position == Some(track.position) {
        return "▶".to_string();
    }
    match table.source {
        TrackSource::Queue { .. } => (track.position + 1).to_string(),
        TrackSource::Library => match track.song.track {
            Some(number) => number.to_string(),
            None => String::new(),
        },
    }
}

fn handle_row_clicks(
    response: &egui::Response,
    table: &TrackTable,
    track: &TrackRow,
    modifiers: egui::Modifiers,
    selection: &mut Selection,
    actions: &mut Vec<Action>,
) {
    if response.clicked() {
        let visible_order = visible_positions(table);
        selection.click(track.position, modifiers, &visible_order);
    }

    if response.secondary_clicked() && !selection.contains(track.position) {
        selection.select_only(track.position);
    }

    if response.double_clicked() {
        match table.source {
            TrackSource::Library => actions.push(Action::PlayNow(vec![track.song.uri.clone()])),
            TrackSource::Queue { tab } => actions.push(Action::PlayInTab {
                tab,
                position: track.position,
            }),
        }
    }
}

fn handle_row_drag(
    ctx: &egui::Context,
    response: &egui::Response,
    table: &TrackTable,
    track: &TrackRow,
    actions: &mut Vec<Action>,
) {
    let TrackSource::Queue { tab } = table.source else {
        return;
    };

    response.dnd_set_drag_payload(DraggedTrack(track.position));

    if response.dnd_hover_payload::<DraggedTrack>().is_some() {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("drop_marker"),
        ));
        let stroke = egui::Stroke::new(2.0, ctx.global_style().visuals.selection.bg_fill);
        painter.hline(response.rect.x_range(), response.rect.top(), stroke);
    }

    if let Some(dragged) = response.dnd_release_payload::<DraggedTrack>() {
        actions.push(Action::MoveInTab {
            tab,
            from: dragged.0,
            to: track.position,
        });
    }
}

fn show_context_menu(
    ui: &mut egui::Ui,
    table: &TrackTable,
    selection: &Selection,
    actions: &mut Vec<Action>,
) {
    if ui.button("Play now").clicked() {
        actions.push(Action::PlayNow(selected_uris(table, selection)));
    }

    ui.menu_button("Add to queue", |ui| {
        for (tab, name) in table.queue_names.iter().enumerate() {
            if table.source == (TrackSource::Queue { tab }) {
                continue;
            }
            if ui.button(name).clicked() {
                actions.push(Action::AddToQueue {
                    tab,
                    uris: selected_uris(table, selection),
                });
            }
        }
        ui.separator();
        if ui.button("New queue…").clicked() {
            actions.push(Action::AskNewQueue {
                name_hint: "Queue".to_string(),
                tracks: selected_uris(table, selection),
            });
        }
    });

    ui.separator();
    if ui
        .button("Scan ReplayGain")
        .on_hover_text("Album gain is measured over the selected tracks of each album")
        .clicked()
    {
        actions.push(Action::ScanReplayGain(selected_uris(table, selection)));
    }

    if let TrackSource::Queue { tab } = table.source {
        ui.separator();
        if ui.button("Remove").clicked() {
            actions.push(Action::RemoveFromTab {
                tab,
                positions: selection.positions(),
            });
        }
    }
}

fn visible_positions(table: &TrackTable) -> Vec<usize> {
    table.rows.iter().map(|row| row.position).collect()
}

fn selected_uris(table: &TrackTable, selection: &Selection) -> Vec<String> {
    let mut uris = Vec::new();
    for row in table.rows {
        if selection.contains(row.position) {
            uris.push(row.song.uri.clone());
        }
    }
    uris
}

#[derive(Default)]
pub struct Selection {
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    pub fn contains(&self, position: usize) -> bool {
        self.selected.contains(&position)
    }

    pub fn positions(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub fn select_all(&mut self, positions: &[usize]) {
        self.selected = positions.iter().copied().collect();
        self.anchor = positions.first().copied();
    }

    pub fn select_only(&mut self, position: usize) {
        self.selected.clear();
        self.selected.insert(position);
        self.anchor = Some(position);
    }

    /// `visible_order` is the order rows are shown in, which Shift-click ranges follow.
    pub fn click(&mut self, position: usize, modifiers: egui::Modifiers, visible_order: &[usize]) {
        if modifiers.shift
            && let Some(range) = self.range_from_anchor(position, visible_order)
        {
            if !modifiers.command {
                self.selected.clear();
            }
            self.selected.extend(range);
            return;
        }

        if modifiers.command {
            if !self.selected.remove(&position) {
                self.selected.insert(position);
            }
            self.anchor = Some(position);
            return;
        }

        self.select_only(position);
    }

    fn range_from_anchor(&self, position: usize, visible_order: &[usize]) -> Option<Vec<usize>> {
        let anchor = self.anchor?;
        let anchor_index = visible_order.iter().position(|&p| p == anchor)?;
        let clicked_index = visible_order.iter().position(|&p| p == position)?;

        let start = anchor_index.min(clicked_index);
        let end = anchor_index.max(clicked_index);
        Some(visible_order[start..=end].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [usize; 5] = [10, 20, 30, 40, 50];

    fn shift() -> egui::Modifiers {
        egui::Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    fn command() -> egui::Modifiers {
        egui::Modifiers {
            command: true,
            ..Default::default()
        }
    }

    #[test]
    fn plain_click_selects_one_row() {
        let mut selection = Selection::default();
        selection.click(20, egui::Modifiers::NONE, &ORDER);
        selection.click(40, egui::Modifiers::NONE, &ORDER);
        assert_eq!(selection.positions(), [40]);
    }

    #[test]
    fn shift_click_selects_a_range_in_visible_order() {
        let mut selection = Selection::default();
        selection.click(40, egui::Modifiers::NONE, &ORDER);
        selection.click(20, shift(), &ORDER);
        assert_eq!(selection.positions(), [20, 30, 40]);
    }

    #[test]
    fn select_all_replaces_the_selection() {
        let mut selection = Selection::default();
        selection.click(10, egui::Modifiers::NONE, &ORDER);
        selection.select_all(&[20, 40]);
        assert_eq!(selection.positions(), [20, 40]);
    }

    #[test]
    fn command_click_toggles_rows() {
        let mut selection = Selection::default();
        selection.click(10, egui::Modifiers::NONE, &ORDER);
        selection.click(30, command(), &ORDER);
        selection.click(10, command(), &ORDER);
        assert_eq!(selection.positions(), [30]);
    }
}
