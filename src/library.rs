use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Song {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub date: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration: Option<Duration>,
}

impl Song {
    pub fn from_uri(uri: &str) -> Song {
        Song {
            uri: uri.to_string(),
            ..Song::default()
        }
    }

    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            file_name(&self.uri)
        } else {
            &self.title
        }
    }

    pub fn directory(&self) -> &str {
        parent_directory(&self.uri)
    }

    pub fn album_artist_or_artist(&self) -> &str {
        if self.album_artist.is_empty() {
            &self.artist
        } else {
            &self.album_artist
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Duration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SongSort {
    pub column: SortColumn,
    pub descending: bool,
}

impl SongSort {
    /// Clicking a column sorts it ascending, then descending, then not at all.
    pub fn after_click(current: Option<SongSort>, column: SortColumn) -> Option<SongSort> {
        match current {
            Some(sort) if sort.column == column && !sort.descending => Some(SongSort {
                column,
                descending: true,
            }),
            Some(sort) if sort.column == column => None,
            _ => Some(SongSort {
                column,
                descending: false,
            }),
        }
    }

    pub fn compare(&self, a: &Song, b: &Song) -> Ordering {
        let ordering = match self.column {
            SortColumn::Title => compare_ignoring_case(a.display_title(), b.display_title()),
            SortColumn::Artist => compare_ignoring_case(&a.artist, &b.artist),
            SortColumn::Album => compare_ignoring_case(&a.album, &b.album)
                .then(a.disc.cmp(&b.disc))
                .then(a.track.cmp(&b.track)),
            SortColumn::Duration => a.duration.cmp(&b.duration),
        };
        if self.descending {
            ordering.reverse()
        } else {
            ordering
        }
    }
}

/// Compares without allocating, since track views re-sort their rows on every repaint.
fn compare_ignoring_case(a: &str, b: &str) -> Ordering {
    let a = a.chars().flat_map(char::to_lowercase);
    let b = b.chars().flat_map(char::to_lowercase);
    a.cmp(b)
}

/// A group of songs the library can be browsed by.
#[derive(Clone, Debug, PartialEq)]
pub enum Collection {
    All,
    Directory(String),
    Artist(String),
    Album(Album),
    Genre(String),
}

impl Collection {
    pub fn title(&self) -> &str {
        match self {
            Collection::All => "All music",
            Collection::Directory(path) => path,
            Collection::Artist(artist) => or_unknown(artist, "Unknown artist"),
            Collection::Album(album) => album.title(),
            Collection::Genre(genre) => or_unknown(genre, "Unknown genre"),
        }
    }

    /// A short name for a queue made from this collection.
    pub fn queue_name(&self) -> &str {
        match self {
            Collection::Directory(path) => file_name(path),
            other => other.title(),
        }
    }

    fn contains(&self, song: &Song) -> bool {
        match self {
            Collection::All => true,
            Collection::Directory(path) => song
                .uri
                .strip_prefix(path.as_str())
                .is_some_and(|rest| rest.starts_with('/')),
            Collection::Artist(artist) => song.album_artist_or_artist() == artist,
            Collection::Album(album) => {
                song.album_artist_or_artist() == album.artist && song.album == album.name
            }
            Collection::Genre(genre) => song.genre == *genre,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Album {
    pub artist: String,
    pub name: String,
}

impl Album {
    pub fn title(&self) -> &str {
        or_unknown(&self.name, "Unknown album")
    }

    pub fn artist_title(&self) -> &str {
        or_unknown(&self.artist, "Unknown artist")
    }
}

fn or_unknown<'a>(value: &'a str, unknown: &'a str) -> &'a str {
    if value.is_empty() { unknown } else { value }
}

pub fn file_name(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, name)) => name,
        None => path,
    }
}

pub fn parent_directory(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Directory {
    pub name: String,
    pub path: String,
    pub children: Vec<Directory>,
}

#[derive(Default)]
pub struct Library {
    songs: Vec<Song>,
    index_by_uri: HashMap<String, usize>,
    root: Directory,
    directories: Vec<String>,
    artists: Vec<String>,
    albums: Vec<Album>,
    genres: Vec<String>,
}

impl Library {
    pub fn new(mut songs: Vec<Song>) -> Library {
        songs.sort_by(|a, b| a.uri.cmp(&b.uri));

        let mut index_by_uri = HashMap::new();
        let mut artists = BTreeSet::new();
        let mut albums = BTreeSet::new();
        let mut genres = BTreeSet::new();
        for (index, song) in songs.iter().enumerate() {
            index_by_uri.insert(song.uri.clone(), index);
            artists.insert(song.album_artist_or_artist());
            albums.insert((song.album.as_str(), song.album_artist_or_artist()));
            genres.insert(song.genre.as_str());
        }

        let mut albums: Vec<Album> = albums
            .into_iter()
            .map(|(name, artist)| Album {
                artist: artist.to_string(),
                name: name.to_string(),
            })
            .collect();
        albums.sort_by_cached_key(|album| (album.name.to_lowercase(), album.artist.to_lowercase()));

        let artists = sorted_ignoring_case(artists);
        let genres = sorted_ignoring_case(genres);
        let root = build_directory_tree(&songs);
        let mut directories = Vec::new();
        collect_directory_paths(&root, &mut directories);

        Library {
            songs,
            index_by_uri,
            root,
            directories,
            artists,
            albums,
            genres,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn root(&self) -> &Directory {
        &self.root
    }

    /// Every directory path, in tree order.
    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    pub fn artists(&self) -> &[String] {
        &self.artists
    }

    pub fn albums(&self) -> &[Album] {
        &self.albums
    }

    pub fn genres(&self) -> &[String] {
        &self.genres
    }

    pub fn song(&self, index: usize) -> &Song {
        &self.songs[index]
    }

    pub fn find(&self, uri: &str) -> Option<&Song> {
        let index = self.index_by_uri.get(uri)?;
        Some(&self.songs[*index])
    }

    /// Queues may hold songs that have since left the library; those are shown by file name.
    pub fn song_or_placeholder(&self, uri: &str) -> Cow<'_, Song> {
        match self.find(uri) {
            Some(song) => Cow::Borrowed(song),
            None => Cow::Owned(Song::from_uri(uri)),
        }
    }

    /// Folders keep path order; artists, albums and genres are ordered by album, disc and track.
    pub fn songs_in(&self, collection: &Collection) -> Vec<usize> {
        let mut indices = Vec::new();
        for (index, song) in self.songs.iter().enumerate() {
            if collection.contains(song) {
                indices.push(index);
            }
        }

        if matches!(
            collection,
            Collection::Artist(_) | Collection::Album(_) | Collection::Genre(_)
        ) {
            indices.sort_by_key(|&index| {
                let song = &self.songs[index];
                (&song.album, song.disc, song.track, &song.uri)
            });
        }
        indices
    }

    pub fn uris_in(&self, collection: &Collection) -> Vec<String> {
        let mut uris = Vec::new();
        for index in self.songs_in(collection) {
            uris.push(self.songs[index].uri.clone());
        }
        uris
    }
}

fn sorted_ignoring_case(values: BTreeSet<&str>) -> Vec<String> {
    let mut values: Vec<String> = values.into_iter().map(str::to_string).collect();
    values.sort_by_cached_key(|value| value.to_lowercase());
    values
}

fn collect_directory_paths(directory: &Directory, paths: &mut Vec<String>) {
    for child in &directory.children {
        paths.push(child.path.clone());
        collect_directory_paths(child, paths);
    }
}

fn build_directory_tree(songs: &[Song]) -> Directory {
    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
    }

    let mut root = Node::default();
    for song in songs {
        let mut node = &mut root;
        for component in song.directory().split('/') {
            if component.is_empty() {
                continue;
            }
            node = node.children.entry(component.to_string()).or_default();
        }
    }

    fn convert(name: String, path: String, node: Node) -> Directory {
        let mut children = Vec::new();
        for (child_name, child_node) in node.children {
            let child_path = if path.is_empty() {
                child_name.clone()
            } else {
                format!("{path}/{child_name}")
            };
            children.push(convert(child_name, child_path, child_node));
        }
        Directory {
            name,
            path,
            children,
        }
    }

    convert(String::new(), String::new(), root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_of(uris: &[&str]) -> Library {
        let mut songs = Vec::new();
        for uri in uris {
            songs.push(Song::from_uri(uri));
        }
        Library::new(songs)
    }

    #[test]
    fn builds_directory_tree_from_song_paths() {
        let library = library_of(&[
            "Music/Jazz/a.flac",
            "Music/Blues/b.mp3",
            "Library/Alpha/Morning/01.flac",
            "loose.mp3",
        ]);

        let root = library.root();
        let top_level: Vec<&str> = root.children.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(top_level, ["Library", "Music"]);

        let music = &root.children[1];
        let subfolders: Vec<&str> = music.children.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(subfolders, ["Music/Blues", "Music/Jazz"]);

        let alpha = &root.children[0].children[0];
        assert_eq!(alpha.children[0].path, "Library/Alpha/Morning");

        assert_eq!(
            library.directories(),
            [
                "Library",
                "Library/Alpha",
                "Library/Alpha/Morning",
                "Music",
                "Music/Blues",
                "Music/Jazz",
            ]
        );
    }

    #[test]
    fn songs_in_directory_are_recursive_and_exact() {
        let library = library_of(&[
            "Music/Jazz/a.flac",
            "Music/Jazz/sub/b.flac",
            "Music/Jazz2/c.flac",
        ]);

        assert_eq!(
            library.uris_in(&Collection::Directory("Music/Jazz".to_string())),
            ["Music/Jazz/a.flac", "Music/Jazz/sub/b.flac"]
        );
        assert_eq!(library.uris_in(&Collection::All).len(), 3);
    }

    fn tagged(uri: &str, artist: &str, album: &str, track: u32, genre: &str) -> Song {
        Song {
            uri: uri.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            track: Some(track),
            genre: genre.to_string(),
            ..Song::default()
        }
    }

    fn tagged_library() -> Library {
        let mut compilation_track = tagged("c/1.flac", "Guest", "Hits", 1, "Pop");
        compilation_track.album_artist = "Various".to_string();
        Library::new(vec![
            tagged("a/2.flac", "alpha", "Morning", 2, "Jazz"),
            tagged("a/1.flac", "alpha", "Morning", 1, "Jazz"),
            tagged("b/1.flac", "Bravo", "Hits", 1, "Rock"),
            compilation_track,
            tagged("loose.mp3", "", "", 1, ""),
        ])
    }

    #[test]
    fn lists_artists_albums_and_genres_sorted_ignoring_case() {
        let library = tagged_library();

        assert_eq!(library.artists(), ["", "alpha", "Bravo", "Various"]);
        assert_eq!(library.genres(), ["", "Jazz", "Pop", "Rock"]);

        let albums: Vec<(&str, &str)> = library
            .albums()
            .iter()
            .map(|album| (album.name.as_str(), album.artist.as_str()))
            .collect();
        assert_eq!(
            albums,
            [
                ("", ""),
                ("Hits", "Bravo"),
                ("Hits", "Various"),
                ("Morning", "alpha")
            ]
        );
    }

    #[test]
    fn tag_collections_are_ordered_by_track() {
        let library = tagged_library();

        let morning = Collection::Album(Album {
            artist: "alpha".to_string(),
            name: "Morning".to_string(),
        });
        assert_eq!(library.uris_in(&morning), ["a/1.flac", "a/2.flac"]);

        let various = Collection::Artist("Various".to_string());
        assert_eq!(library.uris_in(&various), ["c/1.flac"]);

        let rock = Collection::Genre("Rock".to_string());
        assert_eq!(library.uris_in(&rock), ["b/1.flac"]);
    }

    #[test]
    fn clicking_a_column_cycles_ascending_descending_off() {
        let ascending = SongSort::after_click(None, SortColumn::Artist);
        assert_eq!(
            ascending,
            Some(SongSort {
                column: SortColumn::Artist,
                descending: false
            })
        );

        let descending = SongSort::after_click(ascending, SortColumn::Artist);
        assert!(descending.is_some_and(|sort| sort.descending));
        assert_eq!(SongSort::after_click(descending, SortColumn::Artist), None);

        let other_column = SongSort::after_click(descending, SortColumn::Title);
        assert_eq!(
            other_column,
            Some(SongSort {
                column: SortColumn::Title,
                descending: false
            })
        );
    }

    fn sorted_uris(songs: &[Song], column: SortColumn, descending: bool) -> Vec<&str> {
        let sort = SongSort { column, descending };
        let mut songs: Vec<&Song> = songs.iter().collect();
        songs.sort_by(|a, b| sort.compare(a, b));
        songs.iter().map(|song| song.uri.as_str()).collect()
    }

    #[test]
    fn sorting_by_artist_ignores_case() {
        let songs = [
            tagged("1", "bravo", "", 1, ""),
            tagged("2", "Alpha", "", 1, ""),
            tagged("3", "Charlie", "", 1, ""),
        ];
        assert_eq!(
            sorted_uris(&songs, SortColumn::Artist, false),
            ["2", "1", "3"]
        );
    }

    #[test]
    fn sorting_by_album_keeps_track_order() {
        let songs = [
            tagged("b2", "", "Beta", 2, ""),
            tagged("a1", "", "alpha", 1, ""),
            tagged("b1", "", "Beta", 1, ""),
        ];
        assert_eq!(
            sorted_uris(&songs, SortColumn::Album, false),
            ["a1", "b1", "b2"]
        );
    }

    #[test]
    fn descending_sort_keeps_ties_in_their_original_order() {
        let songs = [
            tagged("first", "Same", "", 1, ""),
            tagged("other", "Alpha", "", 1, ""),
            tagged("second", "same", "", 1, ""),
        ];
        assert_eq!(
            sorted_uris(&songs, SortColumn::Artist, true),
            ["first", "second", "other"]
        );
    }

    #[test]
    fn untagged_collections_have_readable_titles() {
        assert_eq!(Collection::Artist(String::new()).title(), "Unknown artist");
        assert_eq!(
            Collection::Directory("Music/Jazz".to_string()).queue_name(),
            "Jazz"
        );
    }
}
