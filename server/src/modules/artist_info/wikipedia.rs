//! Finds an Artist's Wikipedia summary: MusicBrainz names the Artist (through
//! one of their Albums), its Wikidata link names the Wikipedia page, and the
//! page summary is the biography (ADR 0027).

use super::fetch;
use super::repository::Biography;
use crate::observability;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const ALBUM_ATTEMPTS: usize = 2;
const FALLBACK_LANGUAGE: &str = "en";
const MUSICBRAINZ_BACKOFF: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(4)];

/// MusicBrainz allows one request per second per client.
#[derive(Default)]
pub struct Pacer(Mutex<Option<Instant>>);

impl Pacer {
    async fn wait(&self) {
        let started = Instant::now();
        let mut last = self.0.lock().await;
        if let Some(previous) = *last {
            let next = previous + Duration::from_secs(1);
            tokio::time::sleep_until(next.into()).await;
        }
        *last = Some(Instant::now());
        observability::record_span(started, "artist_info.wait", started.elapsed(), &[], &[]);
    }
}

pub struct Endpoints<'a> {
    pub musicbrainz: &'a str,
    pub wikidata: &'a str,
    /// With `{lang}` where the language subdomain goes.
    pub wikipedia: &'a str,
}

pub async fn find_biography(
    client: &reqwest::Client,
    endpoints: &Endpoints<'_>,
    pacer: &Pacer,
    name: &str,
    album_titles: &[String],
    language: &str,
) -> Result<Option<Biography>, String> {
    let Some(mbid) =
        musicbrainz_artist(client, endpoints.musicbrainz, pacer, name, album_titles).await?
    else {
        return Ok(None);
    };
    let artist = musicbrainz(
        client,
        pacer,
        &format!("{}/ws/2/artist/{mbid}", endpoints.musicbrainz),
        &[("inc", "url-rels"), ("fmt", "json")],
    )
    .await?;
    let Some(entity) = wikidata_entity(&artist) else {
        return Ok(None);
    };
    let entity_data: Value = fetch::json(
        client,
        &format!(
            "{}/wiki/Special:EntityData/{entity}.json",
            endpoints.wikidata
        ),
        &[],
    )
    .await?;
    let Some((language, title)) = wikipedia_title(&entity_data, &entity, language) else {
        return Ok(None);
    };
    let mut url = url::Url::parse(&endpoints.wikipedia.replace("{lang}", &language))
        .map_err(|error| format!("Wikipedia URL: {error}"))?;
    url.path_segments_mut()
        .map_err(|_| "Wikipedia URL cannot have a path".to_owned())?
        .extend([
            "api",
            "rest_v1",
            "page",
            "summary",
            &title.replace(' ', "_"),
        ]);
    let summary: Value = fetch::json(client, url.as_str(), &[]).await?;
    Ok(biography(&summary))
}

/// The MBID of the Artist credited on one of their Albums, else of the only
/// exact match for the name.
async fn musicbrainz_artist(
    client: &reqwest::Client,
    base_url: &str,
    pacer: &Pacer,
    name: &str,
    album_titles: &[String],
) -> Result<Option<String>, String> {
    for title in album_titles.iter().take(ALBUM_ATTEMPTS) {
        let query = format!("releasegroup:{} AND artist:{}", phrase(title), phrase(name));
        let groups = musicbrainz(
            client,
            pacer,
            &format!("{base_url}/ws/2/release-group"),
            &[("query", &query), ("fmt", "json"), ("limit", "5")],
        )
        .await?;
        if let Some(mbid) = credited_artist(&groups, name) {
            return Ok(Some(mbid));
        }
    }
    let query = format!("artist:{}", phrase(name));
    let artists = musicbrainz(
        client,
        pacer,
        &format!("{base_url}/ws/2/artist"),
        &[("query", &query), ("fmt", "json"), ("limit", "5")],
    )
    .await?;
    Ok(only_exact_artist(&artists, name))
}

/// A paced MusicBrainz GET. MusicBrainz sheds load with 503 ("try again")
/// when its server-wide budget runs low, so a 503 is retried after 2 s and
/// then 4 s before the lookup counts as failed.
async fn musicbrainz(
    client: &reqwest::Client,
    pacer: &Pacer,
    url: &str,
    query: &[(&str, &str)],
) -> Result<Value, String> {
    for backoff in MUSICBRAINZ_BACKOFF {
        pacer.wait().await;
        match fetch::json(client, url, query).await {
            Err(error) if error.status == Some(503) => tokio::time::sleep(backoff).await,
            result => return Ok(result?),
        }
    }
    pacer.wait().await;
    Ok(fetch::json(client, url, query).await?)
}

/// A Lucene phrase; backslashes and quotes are escaped.
fn phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn same_name(a: &str, b: &str) -> bool {
    caseless::default_caseless_match_str(a.trim(), b.trim())
}

fn credited_artist(groups: &Value, name: &str) -> Option<String> {
    groups["release-groups"]
        .as_array()?
        .iter()
        .flat_map(|group| group["artist-credit"].as_array().into_iter().flatten())
        .map(|credit| &credit["artist"])
        .find(|artist| {
            artist["name"]
                .as_str()
                .is_some_and(|found| same_name(found, name))
        })
        .and_then(|artist| artist["id"].as_str())
        .map(str::to_owned)
}

/// Without an Album to go by, a name is trusted only when exactly one
/// MusicBrainz Artist matches it with full score.
fn only_exact_artist(artists: &Value, name: &str) -> Option<String> {
    let mut exact = artists["artists"].as_array()?.iter().filter(|artist| {
        artist["score"].as_i64() == Some(100)
            && artist["name"]
                .as_str()
                .is_some_and(|found| same_name(found, name))
    });
    let only = exact.next()?;
    if exact.next().is_some() {
        return None;
    }
    only["id"].as_str().map(str::to_owned)
}

fn wikidata_entity(artist: &Value) -> Option<String> {
    artist["relations"]
        .as_array()?
        .iter()
        .filter(|relation| relation["type"] == "wikidata")
        .filter_map(|relation| relation["url"]["resource"].as_str())
        .filter_map(|resource| resource.rsplit('/').next())
        .find(|entity| entity.starts_with('Q') && entity[1..].chars().all(|c| c.is_ascii_digit()))
        .map(str::to_owned)
}

/// The page title in `language`, else in English, with the language used.
fn wikipedia_title(entity_data: &Value, entity: &str, language: &str) -> Option<(String, String)> {
    let sitelinks = &entity_data["entities"][entity]["sitelinks"];
    [language, FALLBACK_LANGUAGE]
        .into_iter()
        .find_map(|language| {
            sitelinks[format!("{language}wiki")]["title"]
                .as_str()
                .map(|title| (language.to_owned(), title.to_owned()))
        })
}

fn biography(summary: &Value) -> Option<Biography> {
    if summary["type"] == "disambiguation" {
        return None;
    }
    let text = summary["extract"].as_str()?.trim();
    let source_url = summary["content_urls"]["desktop"]["page"].as_str()?;
    (!text.is_empty()).then(|| Biography {
        text: text.to_owned(),
        source_url: source_url.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn takes_the_artist_credited_on_the_album() {
        let groups = json!({"release-groups": [
            {"artist-credit": [{"artist": {"id": "other", "name": "Someone"}}]},
            {"artist-credit": [{"artist": {"id": "mbid-1", "name": "SKILLET"}}]}
        ]});
        assert_eq!(
            credited_artist(&groups, "Skillet").as_deref(),
            Some("mbid-1")
        );
        assert_eq!(credited_artist(&groups, "Nobody"), None);
    }

    #[test]
    fn trusts_a_bare_name_only_when_one_artist_matches_fully() {
        let one = json!({"artists": [
            {"id": "a", "name": "Skillet", "score": 100},
            {"id": "b", "name": "Skillet Band", "score": 100},
            {"id": "c", "name": "Skillet", "score": 80}
        ]});
        assert_eq!(only_exact_artist(&one, "Skillet").as_deref(), Some("a"));
        let two = json!({"artists": [
            {"id": "a", "name": "Nirvana", "score": 100},
            {"id": "b", "name": "Nirvana", "score": 100}
        ]});
        assert_eq!(only_exact_artist(&two, "Nirvana"), None);
    }

    #[test]
    fn follows_the_wikidata_link_to_the_page_in_the_language() {
        let artist = json!({"relations": [
            {"type": "official homepage", "url": {"resource": "https://skillet.example"}},
            {"type": "wikidata", "url": {"resource": "https://www.wikidata.org/wiki/Q1155279"}}
        ]});
        assert_eq!(wikidata_entity(&artist).as_deref(), Some("Q1155279"));
        let data = json!({"entities": {"Q1155279": {"sitelinks": {
            "enwiki": {"title": "Skillet (band)"},
            "dewiki": {"title": "Skillet (Band)"}
        }}}});
        assert_eq!(
            wikipedia_title(&data, "Q1155279", "de"),
            Some(("de".to_owned(), "Skillet (Band)".to_owned()))
        );
        assert_eq!(
            wikipedia_title(&data, "Q1155279", "tr"),
            Some(("en".to_owned(), "Skillet (band)".to_owned()))
        );
    }

    #[test]
    fn reads_the_summary_but_not_a_disambiguation_page() {
        let summary = json!({
            "type": "standard",
            "extract": " Skillet is an American Christian rock band. ",
            "content_urls": {"desktop": {"page": "https://en.wikipedia.org/wiki/Skillet_(band)"}}
        });
        assert_eq!(
            biography(&summary),
            Some(Biography {
                text: "Skillet is an American Christian rock band.".to_owned(),
                source_url: "https://en.wikipedia.org/wiki/Skillet_(band)".to_owned(),
            })
        );
        let mut disambiguation = summary.clone();
        disambiguation["type"] = json!("disambiguation");
        assert_eq!(biography(&disambiguation), None);
    }

    #[test]
    fn escapes_lucene_phrases() {
        assert_eq!(phrase(r#"AC\DC "Live""#), r#""AC\\DC \"Live\"""#);
    }
}
