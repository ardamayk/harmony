//! Matches Deezer's most popular Track titles to the Artist's Tracks in the
//! library. Titles are compared without case, bracketed versions ("Live",
//! "2010 Remaster") and " - …" suffixes, so a library Track and Deezer's
//! release of it meet whichever edition either side has.

/// The library Tracks for `titles`, in their order, at most `limit`; a title
/// is shown once, by the Track whose own title matches it best.
pub fn rank<T>(
    titles: &[String],
    tracks: Vec<T>,
    title_of: impl Fn(&T) -> &str,
    limit: usize,
) -> Vec<T> {
    let mut pool: Vec<(String, T)> = tracks
        .into_iter()
        .map(|track| (base_title(title_of(&track)), track))
        .collect();
    let mut seen = Vec::new();
    let mut ranked = Vec::new();
    for title in titles {
        if ranked.len() == limit {
            break;
        }
        let base = base_title(title);
        if base.is_empty() || seen.contains(&base) {
            continue;
        }
        seen.push(base.clone());
        // The plain edition beats a live or remastered one of the same song.
        let best = pool
            .iter()
            .enumerate()
            .filter(|(_, (candidate, _))| *candidate == base)
            .min_by_key(|(_, (_, track))| title_of(track).chars().count())
            .map(|(index, _)| index);
        if let Some(index) = best {
            ranked.push(pool.swap_remove(index).1);
        }
    }
    ranked
}

pub fn base_title(title: &str) -> String {
    let mut text = title.trim();
    loop {
        let trimmed = strip_suffix(text);
        if trimmed == text {
            break;
        }
        text = trimmed;
    }
    folded(text)
}

/// The title without case or extra spaces, versions kept.
pub fn folded(title: &str) -> String {
    let folded = caseless::default_case_fold_str(title.trim());
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One trailing "(…)", "[…]" or " - …" removed, if the rest is not empty.
fn strip_suffix(text: &str) -> &str {
    let text = text.trim_end();
    let cut = if let Some(open) = text.strip_suffix(')').and_then(|_| text.rfind('(')) {
        Some(open)
    } else if let Some(open) = text.strip_suffix(']').and_then(|_| text.rfind('[')) {
        Some(open)
    } else {
        text.rfind(" - ")
    };
    match cut {
        Some(at) if !text[..at].trim().is_empty() => text[..at].trim_end(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (id, title) pairs stand in for library Tracks.
    fn ranked(
        titles: &[String],
        tracks: &[(&'static str, &'static str)],
        limit: usize,
    ) -> Vec<&'static str> {
        rank(titles, tracks.to_vec(), |track| track.1, limit)
            .into_iter()
            .map(|track| track.0)
            .collect()
    }

    #[test]
    fn compares_titles_without_case_versions_or_suffixes() {
        assert_eq!(base_title("Altüst Olmuşum"), "altüst olmuşum");
        assert_eq!(base_title("HERO (Live) [2010 Remaster]"), "hero");
        assert_eq!(base_title("Monster - Radio Edit"), "monster");
        assert_eq!(base_title("(Untitled)"), "(untitled)");
    }

    #[test]
    fn keeps_deezer_order_and_shows_each_song_once() {
        let tracks = [
            ("hero-live", "Hero (Live)"),
            ("monster", "Monster"),
            ("hero", "Hero"),
            ("lucy", "Lucy"),
        ];
        let titles =
            ["Monster", "Hero", "Hero (Live)", "Not In Library", "Lucy"].map(str::to_owned);
        assert_eq!(ranked(&titles, &tracks, 10), ["monster", "hero", "lucy"]);
        assert_eq!(ranked(&titles, &tracks, 2), ["monster", "hero"]);
    }
}
