use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::library::SongSort;
use crate::mpd::{Command, PlayerStatus};

pub const UNSAVED_QUEUE_NAME: &str = "MPD queue (unsaved)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    pub name: String,
    pub tracks: Vec<String>,
    pub current: Option<usize>,
    pub elapsed_secs: f64,
}

impl Queue {
    pub fn new(name: &str, tracks: Vec<String>) -> Queue {
        Queue {
            name: name.to_string(),
            tracks,
            current: None,
            elapsed_secs: 0.0,
        }
    }

    fn elapsed(&self) -> Duration {
        Duration::try_from_secs_f64(self.elapsed_secs).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub open_queues: Vec<String>,
    pub active_queue: Option<String>,
}

pub struct QueueStore {
    queues_dir: PathBuf,
    session_file: PathBuf,
}

impl QueueStore {
    pub fn new(data_dir: &Path) -> QueueStore {
        QueueStore {
            queues_dir: data_dir.join("queues"),
            session_file: data_dir.join("session.json"),
        }
    }

    pub fn saved_names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.queues_dir) else {
            return Vec::new();
        };

        let mut names = Vec::new();
        for entry in entries.flatten() {
            if let Ok(queue) = read_json::<Queue>(&entry.path()) {
                names.push(queue.name);
            }
        }
        names.sort_by_key(|name| name.to_lowercase());
        names
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path_for(name).exists()
    }

    pub fn load(&self, name: &str) -> anyhow::Result<Queue> {
        read_json(&self.path_for(name))
    }

    pub fn save(&self, queue: &Queue) -> anyhow::Result<()> {
        write_json(&self.path_for(&queue.name), queue)
    }

    pub fn delete(&self, name: &str) -> anyhow::Result<()> {
        let path = self.path_for(name);
        std::fs::remove_file(&path).with_context(|| format!("cannot delete {}", path.display()))
    }

    pub fn load_session(&self) -> Session {
        read_json(&self.session_file).unwrap_or_default()
    }

    pub fn save_session(&self, session: &Session) -> anyhow::Result<()> {
        write_json(&self.session_file, session)
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.queues_dir
            .join(format!("{}.json", safe_file_name(name)))
    }
}

/// Replaces characters that are not allowed in file names on Windows, macOS or Linux.
fn safe_file_name(name: &str) -> String {
    let mut safe = String::new();
    for character in name.chars() {
        let forbidden = matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        );
        if forbidden || character.is_control() {
            safe.push('_');
        } else {
            safe.push(character);
        }
    }

    let trimmed = safe.trim_end_matches(['.', ' ']).trim_start();
    if trimmed.is_empty() {
        "queue".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let value =
        serde_json::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    Ok(value)
}

/// Writes to a temporary file first so a crash never leaves a half-written file behind.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

pub struct QueueTab {
    pub queue: Queue,
    /// False for a tab that has no file yet, like the one adopted from MPD at startup.
    pub has_file: bool,
    pub unsaved_changes: bool,
    /// Shown right away, but only applied to the tracks once the tab is played.
    pub sort: Option<SongSort>,
}

/// The open queue tabs. At most one is active, and the active tab is kept
/// identical to MPD's play queue. Every method returns the MPD commands
/// needed to keep it that way.
#[derive(Default)]
pub struct OpenQueues {
    tabs: Vec<QueueTab>,
    active: Option<usize>,
    preferred_active_name: Option<String>,
}

impl OpenQueues {
    pub fn tabs(&self) -> &[QueueTab] {
        &self.tabs
    }

    pub fn tab(&self, index: usize) -> &QueueTab {
        &self.tabs[index]
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.queue.name == name)
    }

    pub fn session(&self) -> Session {
        let mut open_queues = Vec::new();
        for tab in &self.tabs {
            if tab.has_file {
                open_queues.push(tab.queue.name.clone());
            }
        }

        let active_queue = match self.active {
            Some(index) => Some(self.tabs[index].queue.name.clone()),
            None => self.preferred_active_name.clone(),
        };

        Session {
            open_queues,
            active_queue,
        }
    }

    /// The active queue from the last session becomes active again once MPD's queue matches it.
    pub fn restore(&mut self, queues: Vec<Queue>, active_name: Option<String>) {
        for queue in queues {
            self.open_saved(queue);
        }
        self.preferred_active_name = active_name;
    }

    pub fn open_saved(&mut self, queue: Queue) -> usize {
        self.push(QueueTab {
            queue,
            has_file: true,
            unsaved_changes: false,
            sort: None,
        })
    }

    pub fn create(&mut self, name: &str, tracks: Vec<String>) -> usize {
        self.push(QueueTab {
            queue: Queue::new(name, tracks),
            has_file: true,
            unsaved_changes: true,
            sort: None,
        })
    }

    fn push(&mut self, tab: QueueTab) -> usize {
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    pub fn close(&mut self, index: usize) {
        self.tabs.remove(index);
        self.active = match self.active {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
    }

    pub fn rename(&mut self, index: usize, new_name: &str) {
        let tab = &mut self.tabs[index];
        tab.queue.name = new_name.to_string();
        tab.has_file = true;
        tab.unsaved_changes = true;
    }

    pub fn play(&mut self, index: usize, position: usize) -> Vec<Command> {
        if self.active == Some(index) {
            return vec![Command::PlayAt(position)];
        }
        self.activate(index, position, Duration::ZERO)
    }

    /// Plays the tab from where it was left off.
    pub fn resume(&mut self, index: usize) -> Vec<Command> {
        if self.active == Some(index) {
            return vec![Command::Play];
        }
        let queue = &self.tabs[index].queue;
        let position = queue.current.unwrap_or(0);
        let elapsed = queue.elapsed();
        self.activate(index, position, elapsed)
    }

    /// The previous active tab keeps its tracks and position; only MPD's queue is replaced.
    fn activate(&mut self, index: usize, position: usize, elapsed: Duration) -> Vec<Command> {
        if let Some(previous) = self.active {
            self.tabs[previous].unsaved_changes = true;
        }

        let tab = &mut self.tabs[index];
        tab.queue.current = Some(position);
        tab.queue.elapsed_secs = elapsed.as_secs_f64();
        tab.unsaved_changes = true;
        self.active = Some(index);

        vec![Command::ReplaceQueue {
            uris: tab.queue.tracks.clone(),
            position,
            elapsed,
        }]
    }

    pub fn append(&mut self, index: usize, uris: Vec<String>) -> Vec<Command> {
        let tab = &mut self.tabs[index];
        tab.queue.tracks.extend(uris.iter().cloned());
        tab.unsaved_changes = true;

        if self.active == Some(index) {
            vec![Command::Append(uris)]
        } else {
            Vec::new()
        }
    }

    pub fn remove(&mut self, index: usize, positions: &[usize]) -> Vec<Command> {
        let mut positions = positions.to_vec();
        positions.sort_unstable();
        positions.dedup();

        let is_active = self.active == Some(index);
        let tab = &mut self.tabs[index];
        let mut commands = Vec::new();

        for &position in positions.iter().rev() {
            if position >= tab.queue.tracks.len() {
                continue;
            }
            tab.queue.tracks.remove(position);
            tab.queue.current = position_after_removal(tab.queue.current, position);
            if is_active {
                commands.push(Command::DeleteAt(position));
            }
        }

        tab.unsaved_changes = true;
        commands
    }

    pub fn move_track(&mut self, index: usize, from: usize, to: usize) -> Vec<Command> {
        let tab = &mut self.tabs[index];
        let length = tab.queue.tracks.len();
        if from >= length || to >= length || from == to {
            return Vec::new();
        }

        let uri = tab.queue.tracks.remove(from);
        tab.queue.tracks.insert(to, uri);
        tab.queue.current = position_after_move(tab.queue.current, from, to);
        tab.unsaved_changes = true;

        if self.active == Some(index) {
            vec![Command::Move { from, to }]
        } else {
            Vec::new()
        }
    }

    pub fn set_sort(&mut self, index: usize, sort: Option<SongSort>) {
        self.tabs[index].sort = sort;
    }

    /// Puts the tracks in `order`, a list of their old positions, and clears the pending sort.
    /// MPD's copy of a reordered active tab is out of date, so the tab is detached and
    /// its next `play` or `resume` replaces MPD's queue.
    pub fn reorder(&mut self, index: usize, order: &[usize]) {
        let tab = &mut self.tabs[index];
        tab.sort = None;
        let unchanged = order.iter().copied().eq(0..order.len());
        if unchanged || order.len() != tab.queue.tracks.len() {
            return;
        }

        let mut tracks = Vec::new();
        for &old_position in order {
            tracks.push(tab.queue.tracks[old_position].clone());
        }
        tab.queue.tracks = tracks;
        tab.queue.current = tab
            .queue
            .current
            .and_then(|current| position_after_reorder(order, current));
        tab.unsaved_changes = true;

        if self.active == Some(index) {
            self.active = None;
        }
    }

    /// Called whenever MPD reports its queue. Changes made by other clients
    /// are mirrored into the active tab, and a queue that no tab knows about
    /// is adopted as an unsaved tab so it is never lost.
    pub fn sync_with_mpd_queue(&mut self, mpd_queue: Vec<String>) {
        match self.active {
            Some(index) => {
                let tab = &mut self.tabs[index];
                if tab.queue.tracks == mpd_queue {
                    return;
                }
                if mpd_queue.is_empty() {
                    self.active = None;
                    return;
                }
                tab.queue.tracks = mpd_queue;
                tab.unsaved_changes = true;
            }
            None => {
                if mpd_queue.is_empty() {
                    return;
                }
                if let Some(index) = self.find_matching_tab(&mpd_queue) {
                    self.active = Some(index);
                    return;
                }
                let index = self.push(QueueTab {
                    queue: Queue::new(UNSAVED_QUEUE_NAME, mpd_queue),
                    has_file: false,
                    unsaved_changes: false,
                    sort: None,
                });
                self.active = Some(index);
            }
        }
    }

    fn find_matching_tab(&self, mpd_queue: &[String]) -> Option<usize> {
        if let Some(name) = &self.preferred_active_name
            && let Some(index) = self.find(name)
            && self.tabs[index].queue.tracks == mpd_queue
        {
            return Some(index);
        }
        self.tabs
            .iter()
            .position(|tab| tab.queue.tracks == mpd_queue)
    }

    pub fn sync_with_mpd_status(&mut self, status: &PlayerStatus) {
        let Some(index) = self.active else {
            return;
        };
        let tab = &mut self.tabs[index];

        if tab.queue.current != status.position {
            tab.queue.current = status.position;
            tab.unsaved_changes = true;
        }
        tab.queue.elapsed_secs = status.elapsed.as_secs_f64();
    }

    /// MPD may change while we're disconnected, so the link is re-established from scratch.
    pub fn forget_active(&mut self) {
        if let Some(index) = self.active {
            self.preferred_active_name = Some(self.tabs[index].queue.name.clone());
        }
        self.active = None;
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.has_file && tab.unsaved_changes)
    }

    /// Saves changed tabs, or every tab with `include_unchanged` (used on exit to keep elapsed times).
    pub fn save(&mut self, store: &QueueStore, include_unchanged: bool) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if !tab.has_file {
                continue;
            }
            if tab.unsaved_changes || include_unchanged {
                store.save(&tab.queue)?;
                tab.unsaved_changes = false;
            }
        }
        Ok(())
    }
}

fn position_after_removal(current: Option<usize>, removed: usize) -> Option<usize> {
    let current = current?;
    if removed < current {
        Some(current - 1)
    } else if removed == current {
        None
    } else {
        Some(current)
    }
}

pub fn position_after_reorder(order: &[usize], old_position: usize) -> Option<usize> {
    order.iter().position(|&position| position == old_position)
}

fn position_after_move(current: Option<usize>, from: usize, to: usize) -> Option<usize> {
    let current = current?;
    if current == from {
        Some(to)
    } else if from < current && current <= to {
        Some(current - 1)
    } else if to <= current && current < from {
        Some(current + 1)
    } else {
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn queue_round_trips_through_json() {
        let directory = std::env::temp_dir().join(format!("mewsic-test-{}", std::process::id()));
        let store = QueueStore::new(&directory);

        let mut queue = Queue::new("Night drive: vol/2", uris(&["a.flac", "b.flac"]));
        queue.current = Some(1);
        queue.elapsed_secs = 42.5;
        store.save(&queue).unwrap();

        assert_eq!(store.load("Night drive: vol/2").unwrap(), queue);
        assert_eq!(store.saved_names(), ["Night drive: vol/2"]);

        store.delete("Night drive: vol/2").unwrap();
        assert!(store.saved_names().is_empty());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn file_names_are_safe_on_every_platform() {
        assert_eq!(safe_file_name("a/b\\c:d*e?"), "a_b_c_d_e_");
        assert_eq!(safe_file_name("trailing. "), "trailing");
        assert_eq!(safe_file_name("///"), "___");
        assert_eq!(safe_file_name(""), "queue");
    }

    #[test]
    fn playing_another_tab_keeps_the_previous_one_intact() {
        let mut queues = OpenQueues::default();
        let first = queues.create("A", uris(&["a1", "a2"]));
        let second = queues.create("B", uris(&["b1", "b2", "b3"]));

        queues.play(first, 1);
        queues.sync_with_mpd_status(&PlayerStatus {
            position: Some(1),
            elapsed: Duration::from_secs(30),
            ..PlayerStatus::default()
        });

        let commands = queues.play(second, 2);

        assert_eq!(queues.active(), Some(second));
        assert!(matches!(
            commands.as_slice(),
            [Command::ReplaceQueue { uris, position: 2, .. }] if uris == &["b1", "b2", "b3"]
        ));

        let previous = &queues.tab(first).queue;
        assert_eq!(previous.tracks, ["a1", "a2"]);
        assert_eq!(previous.current, Some(1));
        assert_eq!(previous.elapsed_secs, 30.0);
    }

    #[test]
    fn resuming_a_tab_seeks_to_where_it_was_left() {
        let mut queues = OpenQueues::default();
        let mut queue = Queue::new("A", uris(&["a1", "a2"]));
        queue.current = Some(1);
        queue.elapsed_secs = 12.0;
        let index = queues.open_saved(queue);

        let commands = queues.resume(index);

        assert!(matches!(
            commands.as_slice(),
            [Command::ReplaceQueue { position: 1, elapsed, .. }] if *elapsed == Duration::from_secs(12)
        ));
    }

    #[test]
    fn only_the_active_tab_sends_edits_to_mpd() {
        let mut queues = OpenQueues::default();
        let active = queues.create("A", uris(&["a1"]));
        let inactive = queues.create("B", uris(&["b1"]));
        queues.play(active, 0);

        assert_eq!(queues.append(inactive, uris(&["b2"])).len(), 0);
        assert_eq!(queues.append(active, uris(&["a2"])).len(), 1);
        assert_eq!(queues.tab(inactive).queue.tracks, ["b1", "b2"]);
    }

    #[test]
    fn unknown_mpd_queue_is_adopted_as_an_unsaved_tab() {
        let mut queues = OpenQueues::default();
        queues.create("A", uris(&["a1"]));

        queues.sync_with_mpd_queue(uris(&["x1", "x2"]));

        let active = queues.active().unwrap();
        assert_eq!(queues.tab(active).queue.name, UNSAVED_QUEUE_NAME);
        assert!(!queues.tab(active).has_file);
        assert_eq!(queues.tab(0).queue.tracks, ["a1"]);
    }

    #[test]
    fn matching_mpd_queue_reactivates_the_restored_tab() {
        let mut queues = OpenQueues::default();
        queues.restore(
            vec![Queue::new("A", uris(&["x"])), Queue::new("B", uris(&["x"]))],
            Some("B".to_string()),
        );

        queues.sync_with_mpd_queue(uris(&["x"]));

        assert_eq!(queues.active(), queues.find("B"));
        assert_eq!(queues.tabs().len(), 2);
    }

    #[test]
    fn clearing_mpd_detaches_instead_of_emptying_the_tab() {
        let mut queues = OpenQueues::default();
        let index = queues.create("A", uris(&["a1"]));
        queues.play(index, 0);

        queues.sync_with_mpd_queue(Vec::new());

        assert_eq!(queues.active(), None);
        assert_eq!(queues.tab(index).queue.tracks, ["a1"]);
    }

    fn title_sort() -> Option<SongSort> {
        Some(SongSort {
            column: crate::library::SortColumn::Title,
            descending: false,
        })
    }

    #[test]
    fn reordering_moves_tracks_and_the_current_position() {
        let mut queues = OpenQueues::default();
        let mut queue = Queue::new("A", uris(&["a", "b", "c"]));
        queue.current = Some(0);
        let index = queues.open_saved(queue);
        queues.set_sort(index, title_sort());

        queues.reorder(index, &[2, 0, 1]);

        let tab = queues.tab(index);
        assert_eq!(tab.queue.tracks, ["c", "a", "b"]);
        assert_eq!(tab.queue.current, Some(1));
        assert_eq!(tab.sort, None);
    }

    #[test]
    fn reordering_the_active_tab_replaces_mpd_queue_on_next_play() {
        let mut queues = OpenQueues::default();
        let index = queues.create("A", uris(&["a", "b"]));
        queues.play(index, 0);

        queues.reorder(index, &[1, 0]);
        let commands = queues.play(index, 0);

        assert!(matches!(
            commands.as_slice(),
            [Command::ReplaceQueue { uris, position: 0, .. }] if uris == &["b", "a"]
        ));
    }

    #[test]
    fn reordering_into_the_same_order_keeps_the_tab_active() {
        let mut queues = OpenQueues::default();
        let index = queues.create("A", uris(&["a", "b"]));
        queues.play(index, 0);
        queues.set_sort(index, title_sort());

        queues.reorder(index, &[0, 1]);

        assert_eq!(queues.active(), Some(index));
        assert_eq!(queues.tab(index).sort, None);
    }

    #[test]
    fn current_track_follows_moves_and_removals() {
        assert_eq!(position_after_move(Some(2), 2, 0), Some(0));
        assert_eq!(position_after_move(Some(2), 0, 3), Some(1));
        assert_eq!(position_after_move(Some(2), 4, 1), Some(3));
        assert_eq!(position_after_removal(Some(2), 0), Some(1));
        assert_eq!(position_after_removal(Some(2), 2), None);
        assert_eq!(position_after_removal(Some(2), 3), Some(2));
    }
}
