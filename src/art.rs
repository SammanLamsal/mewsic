use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use eframe::egui;
use mpd_client::Client;

use crate::library::Song;
use crate::mpd::{Command, MpdHandle};

const MAX_IMAGE_SIZE: u32 = 512;
const CACHED_TEXTURES: usize = 64;

/// Runs on the MPD thread: disk cache first, then MPD's `readpicture`/`albumart`.
pub async fn load(
    client: &Client,
    uri: &str,
    key: &str,
    cache_dir: &Path,
) -> Option<egui::ColorImage> {
    let cache_file = cache_file_for(cache_dir, key);

    let bytes = match std::fs::read(&cache_file) {
        Ok(bytes) => bytes,
        Err(_) => {
            let (bytes, _mime) = client.album_art(uri).await.ok()??;
            let bytes = bytes.to_vec();
            if std::fs::create_dir_all(cache_dir).is_ok() {
                let _ = std::fs::write(&cache_file, &bytes);
            }
            bytes
        }
    };

    tokio::task::spawn_blocking(move || decode(&bytes))
        .await
        .ok()
        .flatten()
}

fn cache_file_for(cache_dir: &Path, key: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    cache_dir.join(format!("{:016x}", hasher.finish()))
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.thumbnail(MAX_IMAGE_SIZE, MAX_IMAGE_SIZE).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

enum Slot {
    Loading,
    Missing,
    Loaded(egui::TextureHandle),
}

/// Album art textures keyed by album (see [`art_key`]), with the oldest entries evicted first.
#[derive(Default)]
pub struct ArtCache {
    slots: HashMap<String, Slot>,
    insertion_order: VecDeque<String>,
}

impl ArtCache {
    /// Returns the texture for the album of `song`, requesting it from MPD if needed.
    pub fn get(&mut self, song: &Song, mpd: &mut MpdHandle) -> Option<&egui::TextureHandle> {
        let key = art_key(song);

        if !self.slots.contains_key(&key) {
            mpd.send(Command::FetchArt {
                key: key.clone(),
                uri: song.uri.clone(),
            });
            self.remember(key.clone(), Slot::Loading);
        }

        match self.slots.get(&key) {
            Some(Slot::Loaded(texture)) => Some(texture),
            _ => None,
        }
    }

    /// Size of the art for `song` if it has already loaded. Never requests it.
    pub fn loaded_size(&self, song: &Song) -> Option<egui::Vec2> {
        match self.slots.get(&art_key(song)) {
            Some(Slot::Loaded(texture)) => Some(texture.size_vec2()),
            _ => None,
        }
    }

    pub fn is_loading(&self, song: &Song) -> bool {
        matches!(self.slots.get(&art_key(song)), Some(Slot::Loading))
    }

    pub fn insert(&mut self, ctx: &egui::Context, key: String, image: Option<egui::ColorImage>) {
        let slot = match image {
            Some(image) => {
                let texture = ctx.load_texture(&key, image, egui::TextureOptions::LINEAR);
                Slot::Loaded(texture)
            }
            None => Slot::Missing,
        };
        self.remember(key, slot);
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.insertion_order.clear();
    }

    fn remember(&mut self, key: String, slot: Slot) {
        let is_new = self.slots.insert(key.clone(), slot).is_none();
        if is_new {
            self.insertion_order.push_back(key);
        }

        while self.insertion_order.len() > CACHED_TEXTURES {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.slots.remove(&oldest);
            }
        }
    }
}

/// Songs of one album share their art, wherever their files are.
/// Untagged songs can't be grouped, so each gets its own.
fn art_key(song: &Song) -> String {
    if song.album.is_empty() {
        song.uri.clone()
    } else {
        format!("{}\0{}", song.album_artist_or_artist(), song.album)
    }
}
