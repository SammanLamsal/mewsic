use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use eframe::egui;

use crate::library::{Song, parent_directory};

const RSGAIN_MISSING: &str =
    "rsgain is not installed or not on PATH (https://github.com/complexlogic/rsgain)";

/// Files measured by one rsgain run. Album groups also get an album gain,
/// computed over exactly these files.
#[derive(Debug, PartialEq)]
pub struct ScanGroup {
    pub uris: Vec<String>,
    pub is_album: bool,
}

pub struct ScanResult {
    /// The directories holding the tagged files, so MPD can re-read just those.
    pub directories: Vec<String>,
    pub errors: Vec<String>,
}

enum ScanEvent {
    GroupFinished,
    Finished(ScanResult),
}

/// Songs of the same album share a group. Songs without an album are tagged one by one,
/// and streams are skipped because there is no file to tag.
pub fn group_by_album(songs: &[&Song]) -> Vec<ScanGroup> {
    let mut albums: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
    let mut singles = Vec::new();

    for song in songs {
        if is_stream(&song.uri) {
            continue;
        }
        if song.album.is_empty() {
            singles.push(ScanGroup {
                uris: vec![song.uri.clone()],
                is_album: false,
            });
        } else {
            let key = (song.album_artist_or_artist(), song.album.as_str());
            albums.entry(key).or_default().push(song.uri.clone());
        }
    }

    let mut groups = Vec::new();
    for uris in albums.into_values() {
        groups.push(ScanGroup {
            uris,
            is_album: true,
        });
    }
    groups.extend(singles);
    groups
}

fn is_stream(uri: &str) -> bool {
    uri.contains("://")
}

/// A ReplayGain scan running on its own threads, since rsgain decodes every file.
pub struct ReplayGainScan {
    events: mpsc::Receiver<ScanEvent>,
    finished_groups: usize,
    group_count: usize,
}

impl ReplayGainScan {
    pub fn start(
        mut groups: Vec<ScanGroup>,
        music_dir: PathBuf,
        ctx: egui::Context,
    ) -> ReplayGainScan {
        // A big album that starts last would leave the scan finishing on a single core.
        groups.sort_by_key(|group| Reverse(group.uris.len()));

        let group_count = groups.len();
        let (sender, receiver) = mpsc::channel();
        let report = move |event: ScanEvent| {
            let _ = sender.send(event);
            ctx.request_repaint();
        };

        std::thread::Builder::new()
            .name("replaygain".to_string())
            .spawn(move || scan(&groups, &music_dir, report))
            .expect("failed to spawn the ReplayGain thread");

        ReplayGainScan {
            events: receiver,
            finished_groups: 0,
            group_count,
        }
    }

    /// Returns the result once every group has been scanned.
    pub fn poll(&mut self) -> Option<ScanResult> {
        for event in self.events.try_iter() {
            match event {
                ScanEvent::GroupFinished => self.finished_groups += 1,
                ScanEvent::Finished(result) => return Some(result),
            }
        }
        None
    }

    /// Finished and total groups, for showing "3 of 12".
    pub fn progress(&self) -> (usize, usize) {
        (self.finished_groups, self.group_count)
    }
}

/// rsgain only uses one core per run, so groups are scanned by one rsgain run per core.
fn scan(groups: &[ScanGroup], music_dir: &Path, report: impl Fn(ScanEvent) + Sync) {
    if let Err(message) = check_rsgain_installed() {
        report(ScanEvent::Finished(ScanResult {
            directories: Vec::new(),
            errors: vec![message],
        }));
        return;
    }

    let next_group = AtomicUsize::new(0);
    let mut outcomes = Vec::new();
    let mut errors = Vec::new();

    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..worker_count(groups.len()) {
            workers.push(scope.spawn(|| scan_worker(groups, music_dir, &next_group, &report)));
        }
        for worker in workers {
            match worker.join() {
                Ok(worker_outcomes) => outcomes.extend(worker_outcomes),
                Err(_) => errors.push("a ReplayGain worker stopped unexpectedly".to_string()),
            }
        }
    });

    let mut directories = BTreeSet::new();
    for (group, outcome) in outcomes {
        match outcome {
            Ok(()) => {
                for uri in &group.uris {
                    directories.insert(parent_directory(uri).to_string());
                }
            }
            Err(message) => errors.push(message),
        }
    }

    report(ScanEvent::Finished(ScanResult {
        directories: directories.into_iter().collect(),
        errors,
    }));
}

fn check_rsgain_installed() -> Result<(), String> {
    match std::process::Command::new("rsgain")
        .arg("--version")
        .output()
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(RSGAIN_MISSING.to_string()),
        Err(error) => Err(format!("cannot run rsgain: {error}")),
    }
}

fn worker_count(group_count: usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    cores.min(group_count)
}

/// Takes groups off the shared list until none are left.
fn scan_worker<'a>(
    groups: &'a [ScanGroup],
    music_dir: &Path,
    next_group: &AtomicUsize,
    report: &impl Fn(ScanEvent),
) -> Vec<(&'a ScanGroup, Result<(), String>)> {
    let mut outcomes = Vec::new();
    loop {
        let index = next_group.fetch_add(1, Ordering::Relaxed);
        let Some(group) = groups.get(index) else {
            return outcomes;
        };
        outcomes.push((group, scan_group(group, music_dir)));
        report(ScanEvent::GroupFinished);
    }
}

fn scan_group(group: &ScanGroup, music_dir: &Path) -> Result<(), String> {
    let output = match run_rsgain(group, music_dir) {
        Ok(output) => output,
        Err(error) => return Err(format!("cannot run rsgain: {error}")),
    };
    if output.status.success() {
        Ok(())
    } else {
        Err(describe_failure(group, &output))
    }
}

/// `custom` mode with `--tagmode=i` writes the tags; without `--album` only track gain is written.
fn run_rsgain(group: &ScanGroup, music_dir: &Path) -> io::Result<Output> {
    let mut command = std::process::Command::new("rsgain");
    command.arg("custom").arg("--tagmode=i");
    if group.is_album {
        command.arg("--album");
    }
    for uri in &group.uris {
        command.arg(music_dir.join(uri));
    }
    command.output()
}

fn describe_failure(group: &ScanGroup, output: &Output) -> String {
    let location = match group.uris.first() {
        Some(uri) if group.is_album => parent_directory(uri),
        Some(uri) => uri.as_str(),
        None => "",
    };

    let stderr = String::from_utf8_lossy(&output.stderr);
    match first_message(&stderr) {
        Some(message) => format!("rsgain failed on {location}: {message}"),
        None => format!("rsgain failed on {location} ({})", output.status),
    }
}

/// rsgain colours its messages even when they are captured, and starts each with a "[✘]" marker.
fn first_message(stderr: &str) -> Option<String> {
    let plain = strip_ansi_codes(stderr);
    let line = plain.lines().map(str::trim).find(|line| !line.is_empty())?;
    let without_marker = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
        .map(|(_, message)| message);
    Some(without_marker.unwrap_or(line).to_string())
}

/// Removes escape sequences such as `ESC[1;31m`, which end at their first letter.
fn strip_ansi_codes(text: &str) -> String {
    let mut plain = String::new();
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            in_escape = !character.is_ascii_alphabetic();
        } else if character == '\x1b' {
            in_escape = true;
        } else {
            plain.push(character);
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(uri: &str, artist: &str, album: &str) -> Song {
        Song {
            uri: uri.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            ..Song::default()
        }
    }

    #[test]
    fn groups_songs_by_album_and_tags_loose_songs_alone() {
        let songs = [
            song("a/1.flac", "Alpha", "Morning"),
            song("loose.mp3", "Alpha", ""),
            song("a/2.flac", "Alpha", "Morning"),
            song("b/1.flac", "Bravo", "Morning"),
            song("http://radio.example/stream", "Radio", "Live"),
        ];
        let references: Vec<&Song> = songs.iter().collect();

        let groups = group_by_album(&references);

        assert_eq!(
            groups,
            [
                ScanGroup {
                    uris: vec!["a/1.flac".to_string(), "a/2.flac".to_string()],
                    is_album: true,
                },
                ScanGroup {
                    uris: vec!["b/1.flac".to_string()],
                    is_album: true,
                },
                ScanGroup {
                    uris: vec!["loose.mp3".to_string()],
                    is_album: false,
                },
            ]
        );
    }

    #[test]
    fn reports_the_first_rsgain_message_without_colours() {
        let stderr = "[\x1b[1;31m✘\x1b[0m] File 'missing.flac' does not exist\n\
                      [\x1b[1;31m✘\x1b[0m] File list is not valid\n";
        assert_eq!(
            first_message(stderr).as_deref(),
            Some("File 'missing.flac' does not exist")
        );
        assert_eq!(first_message("\n  \n"), None);
    }
}
