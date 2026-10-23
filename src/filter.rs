use crate::library::Song;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Any,
    Title,
    Artist,
    AlbumArtist,
    Album,
    Genre,
    Date,
    Path,
}

impl Field {
    fn from_name(name: &str) -> Option<Field> {
        match name.to_lowercase().as_str() {
            "title" => Some(Field::Title),
            "artist" => Some(Field::Artist),
            "albumartist" => Some(Field::AlbumArtist),
            "album" => Some(Field::Album),
            "genre" => Some(Field::Genre),
            "date" | "year" => Some(Field::Date),
            "path" | "file" => Some(Field::Path),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
struct Term {
    field: Field,
    text: String,
    negated: bool,
}

impl Term {
    fn matches(&self, song: &Song) -> bool {
        let found = match self.field {
            Field::Any => {
                contains(song.display_title(), &self.text)
                    || contains(&song.artist, &self.text)
                    || contains(&song.album_artist, &self.text)
                    || contains(&song.album, &self.text)
                    || contains(&song.uri, &self.text)
            }
            Field::Title => contains(song.display_title(), &self.text),
            Field::Artist => contains(&song.artist, &self.text),
            Field::AlbumArtist => contains(&song.album_artist, &self.text),
            Field::Album => contains(&song.album, &self.text),
            Field::Genre => contains(&song.genre, &self.text),
            Field::Date => contains(&song.date, &self.text),
            Field::Path => contains(&song.uri, &self.text),
        };

        found != self.negated
    }
}

fn contains(haystack: &str, lowercase_needle: &str) -> bool {
    haystack.to_lowercase().contains(lowercase_needle)
}

/// A filter like `alpha album:"morning" -live`: every term must match.
#[derive(Debug, Default, PartialEq)]
pub struct Query {
    terms: Vec<Term>,
}

impl Query {
    pub fn parse(input: &str) -> Query {
        let mut terms = Vec::new();
        for word in split_words(input) {
            if let Some(term) = parse_term(&word) {
                terms.push(term);
            }
        }
        Query { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, song: &Song) -> bool {
        self.terms.iter().all(|term| term.matches(song))
    }
}

fn split_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for character in input.chars() {
        if character == '"' {
            in_quotes = !in_quotes;
        } else if character.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }

    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn parse_term(word: &str) -> Option<Term> {
    let (negated, word) = match word.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, word),
    };

    let mut field = Field::Any;
    let mut text = word;
    if let Some((name, value)) = word.split_once(':')
        && let Some(known_field) = Field::from_name(name)
    {
        field = known_field;
        text = value;
    }

    if text.is_empty() {
        return None;
    }

    Some(Term {
        field,
        text: text.to_lowercase(),
        negated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: &str, album: &str) -> Song {
        Song {
            uri: format!("{artist}/{album}/{title}.flac"),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            ..Song::default()
        }
    }

    #[test]
    fn bare_words_match_any_field_case_insensitively() {
        let query = Query::parse("ALPHA hello");
        assert!(query.matches(&song("Hello", "Alpha", "Morning")));
        assert!(!query.matches(&song("Goodbye", "Alpha", "Morning")));
    }

    #[test]
    fn field_prefixes_restrict_matching() {
        let query = Query::parse("album:morning");
        assert!(query.matches(&song("Goodbye", "Alpha", "Morning")));
        assert!(!query.matches(&song("Morning", "Alpha", "Other")));
    }

    #[test]
    fn quotes_group_words() {
        let query = Query::parse("artist:\"night owls\"");
        assert!(query.matches(&song("Moonlight", "The Night Owls", "Sky")));
        assert!(!query.matches(&song("Owls", "Night", "x")));
    }

    #[test]
    fn minus_negates_a_term() {
        let query = Query::parse("alpha -album:live");
        assert!(query.matches(&song("Goodbye", "Alpha", "Morning")));
        assert!(!query.matches(&song("Goodbye", "Alpha", "Live in Paris")));
    }

    #[test]
    fn unknown_prefix_is_treated_as_plain_text() {
        let query = Query::parse("re:mix");
        assert!(query.matches(&song("Re:Mix Session", "x", "y")));
    }

    #[test]
    fn empty_input_matches_everything() {
        let query = Query::parse("   ");
        assert!(query.is_empty());
        assert!(query.matches(&song("a", "b", "c")));
    }
}
