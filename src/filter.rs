use unicode_normalization::char::{decompose_compatible, is_combining_mark};

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
    needle: Needle,
    negated: bool,
}

impl Term {
    fn matches(&self, song: &Song) -> bool {
        let needle = &self.needle;
        let found = match self.field {
            Field::Any => {
                needle.is_in(song.display_title())
                    || needle.is_in(&song.artist)
                    || needle.is_in(&song.album_artist)
                    || needle.is_in(&song.album)
                    || needle.is_in(&song.uri)
            }
            Field::Title => needle.is_in(song.display_title()),
            Field::Artist => needle.is_in(&song.artist),
            Field::AlbumArtist => needle.is_in(&song.album_artist),
            Field::Album => needle.is_in(&song.album),
            Field::Genre => needle.is_in(&song.genre),
            Field::Date => needle.is_in(&song.date),
            Field::Path => needle.is_in(&song.uri),
        };

        found != self.negated
    }
}

/// One word or phrase to search for, compared without regard to case, accents, or punctuation.
#[derive(Debug, PartialEq)]
pub struct Needle {
    normalized: String,
    literal: String,
}

impl Needle {
    pub fn new(text: &str) -> Needle {
        Needle {
            normalized: normalize(text),
            literal: text.to_lowercase(),
        }
    }

    pub fn is_in(&self, haystack: &str) -> bool {
        // A needle of only punctuation, like the band "!!!", normalizes to nothing and would match
        // everything, so it is searched for literally instead.
        if self.normalized.is_empty() {
            return haystack.to_lowercase().contains(&self.literal);
        }
        normalize(haystack).contains(&self.normalized)
    }
}

/// Lowercases `text` and strips accents and punctuation, so "Don’t Stop" becomes "dont stop".
///
/// Punctuation is dropped rather than turned into spaces so that "dont" finds "don't" and "acdc"
/// finds "AC/DC".
pub fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;

    for character in text.chars() {
        decompose_compatible(character, |part| {
            if part.is_whitespace() {
                pending_space = !normalized.is_empty();
            } else if part.is_alphanumeric() && !is_combining_mark(part) {
                if pending_space {
                    normalized.push(' ');
                    pending_space = false;
                }
                normalized.extend(part.to_lowercase());
            }
        });
    }
    normalized
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
        needle: Needle::new(text),
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
    fn apostrophes_are_optional_on_either_side() {
        assert!(Query::parse("dont").matches(&song("Don't Stop", "x", "y")));
        assert!(Query::parse("dont").matches(&song("Don’t Stop", "x", "y")));
        assert!(Query::parse("don't").matches(&song("Dont Stop", "x", "y")));
        assert!(Query::parse("don't").matches(&song("Don’t Stop", "x", "y")));
    }

    #[test]
    fn stylized_punctuation_is_ignored() {
        let query = Query::parse("rock n roll");
        assert!(query.matches(&song("Rock 'n' Roll", "x", "y")));
        assert!(query.matches(&song("Rock'n'Roll", "x", "y")));
        assert!(Query::parse("guns n roses").matches(&song("x", "Guns N' Roses", "y")));
        assert!(Query::parse("til").matches(&song("x", "'Til Tuesday", "y")));
        assert!(Query::parse("acdc").matches(&song("x", "AC/DC", "y")));
        assert!(!Query::parse("dont").matches(&song("Do Not", "x", "y")));
    }

    #[test]
    fn accents_are_ignored() {
        assert!(Query::parse("beyonce").matches(&song("x", "Beyoncé", "y")));
        assert!(Query::parse("artist:\"sigur ros\"").matches(&song("x", "Sigur Rós", "y")));
        assert!(Query::parse("Björk").matches(&song("x", "Bjork", "y")));
    }

    #[test]
    fn punctuation_only_terms_match_literally() {
        let query = Query::parse("!!!");
        assert!(query.matches(&song("x", "!!!", "y")));
        assert!(!query.matches(&song("x", "Alpha", "y")));
    }

    #[test]
    fn normalize_collapses_whitespace_and_drops_punctuation() {
        assert_eq!(normalize("  Rock  'n'   Roll! "), "rock n roll");
        assert_eq!(normalize("Ｆｕｌｌ ﬁve"), "full five");
    }

    #[test]
    fn empty_input_matches_everything() {
        let query = Query::parse("   ");
        assert!(query.is_empty());
        assert!(query.matches(&song("a", "b", "c")));
    }
}
