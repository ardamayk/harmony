//! Finds an Artist's photo and most popular Track titles on Deezer's public
//! API (no key). The Artist is found through one of their Albums, so two
//! Artists sharing a name resolve to the one on the user's Album (ADR 0027).

use super::fetch;
use serde::Deserialize;
use serde_json::Value;

/// How many of the Artist's Albums are tried before the name search.
const ALBUM_ATTEMPTS: usize = 3;
/// Deezer's empty-picture URLs have no image id between the slashes.
const PLACEHOLDER_PATH: &str = "/images/artist//";
/// How many top Tracks are asked for; Deezer answers at most 50.
const TOP_TRACKS: &str = "50";

#[derive(Deserialize)]
struct Hits<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    // Deezer answers quota and query errors with HTTP 200 and this object.
    error: Option<Value>,
}

#[derive(Deserialize)]
struct AlbumHit {
    artist: Artist,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Artist {
    #[serde(default)]
    id: i64,
    name: String,
    #[serde(default)]
    picture_big: String,
    #[serde(default)]
    nb_fan: i64,
}

#[derive(Deserialize)]
struct TopTrack {
    #[serde(default)]
    title: String,
    /// The title without its version ("Live", "Remastered"); may be empty.
    #[serde(default)]
    title_short: String,
}

/// The URL of the Artist's 500px picture, `None` when Deezer has none.
pub async fn find_picture(
    client: &reqwest::Client,
    base_url: &str,
    name: &str,
    album_titles: &[String],
) -> Result<Option<String>, String> {
    // A namesake's empty profile can share the Album results.
    let artist = find_artist(client, base_url, name, album_titles, |artist| {
        picture(artist).is_some()
    })
    .await?;
    Ok(artist.and_then(|artist| picture(&artist)))
}

/// The Artist's most popular Track titles on Deezer, best first; empty when
/// Deezer does not know the Artist.
pub async fn find_top_titles(
    client: &reqwest::Client,
    base_url: &str,
    name: &str,
    album_titles: &[String],
) -> Result<Vec<String>, String> {
    let Some(artist) =
        find_artist(client, base_url, name, album_titles, |artist| artist.id > 0).await?
    else {
        return Ok(Vec::new());
    };
    let url = format!("{base_url}/artist/{}/top", artist.id);
    let hits: Hits<TopTrack> = fetch::json(client, &url, &[("limit", TOP_TRACKS)]).await?;
    if let Some(error) = hits.error {
        return Err(format!("GET {url}: {error}"));
    }
    Ok(hits
        .into_iter()
        .map(|track| {
            if track.title_short.trim().is_empty() {
                track.title
            } else {
                track.title_short
            }
        })
        .filter(|title| !title.trim().is_empty())
        .collect())
}

/// The Deezer Artist on one of the user's Albums that `accept`s, else the
/// best name match.
async fn find_artist(
    client: &reqwest::Client,
    base_url: &str,
    name: &str,
    album_titles: &[String],
    accept: impl Fn(&Artist) -> bool,
) -> Result<Option<Artist>, String> {
    // Plain words, not Deezer's `artist:"…"` syntax: that one leaves out
    // well-known Artists (Taylor Swift) while plain words rank them first.
    for title in album_titles.iter().take(ALBUM_ATTEMPTS) {
        let query = format!("{name} {title}");
        let hits: Hits<AlbumHit> = search(client, base_url, "album", &query).await?;
        if let Some(artist) = hits
            .into_iter()
            .map(|hit| hit.artist)
            .find(|artist| same_name(&artist.name, name) && accept(artist))
        {
            return Ok(Some(artist));
        }
    }
    let hits: Hits<Artist> = search(client, base_url, "artist", name).await?;
    Ok(best_by_name(hits.into_iter().collect(), name).filter(|artist| accept(artist)))
}

async fn search<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base_url: &str,
    kind: &str,
    query: &str,
) -> Result<Hits<T>, String> {
    let url = format!("{base_url}/search/{kind}");
    let hits: Hits<T> = fetch::json(client, &url, &[("q", query), ("limit", "10")]).await?;
    match hits.error {
        Some(error) => Err(format!("GET {url}: {error}")),
        None => Ok(hits),
    }
}

impl<T> IntoIterator for Hits<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

fn same_name(a: &str, b: &str) -> bool {
    caseless::default_caseless_match_str(a.trim(), b.trim())
}

/// Deezer's ranking is unreliable for homonyms: an exact-case name first,
/// then a case-insensitive one, then the most fans; no name match, none.
fn best_by_name(artists: Vec<Artist>, name: &str) -> Option<Artist> {
    artists
        .into_iter()
        .filter(|artist| same_name(&artist.name, name))
        .max_by_key(|artist| (artist.name.trim() == name.trim(), artist.nb_fan))
}

fn picture(artist: &Artist) -> Option<String> {
    let url = artist.picture_big.trim();
    (!url.is_empty() && !url.contains(PLACEHOLDER_PATH)).then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artist(name: &str, fans: i64) -> Artist {
        Artist {
            id: 1,
            name: name.to_owned(),
            picture_big: format!("https://cdn.example/images/artist/{name}/500x500.jpg"),
            nb_fan: fans,
        }
    }

    #[test]
    fn prefers_the_exact_case_name_then_the_most_fans() {
        let found = best_by_name(
            vec![
                artist("Other", 900),
                artist("SKILLET", 500),
                artist("Skillet", 10),
                artist("Skillet", 20),
            ],
            "Skillet",
        );
        assert_eq!(found, Some(artist("Skillet", 20)));
        let found = best_by_name(vec![artist("SKILLET", 5), artist("Other", 9)], "Skillet");
        assert_eq!(found, Some(artist("SKILLET", 5)));
        assert_eq!(best_by_name(vec![artist("Other", 9)], "Skillet"), None);
    }

    #[test]
    fn treats_the_empty_picture_as_none() {
        let mut empty = artist("Skillet", 1);
        empty.picture_big =
            "https://cdn.example/images/artist//500x500-000000-80-0-0.jpg".to_owned();
        assert_eq!(picture(&empty), None);
        assert!(picture(&artist("Skillet", 1)).is_some());
    }

    #[test]
    fn reads_deezer_errors_returned_with_http_200() {
        let hits: Hits<Artist> =
            serde_json::from_str(r#"{"error":{"type":"Exception","code":4}}"#).expect("hits");
        assert!(hits.error.is_some());
        assert!(hits.data.is_empty());
    }
}
