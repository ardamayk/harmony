//! Recomputes every Library Search text the Go server wrote and expects the
//! same bytes. Point EARTHLY_PARITY_DB at a copy of a real database:
//!
//!   EARTHLY_PARITY_DB=/tmp/app-copy.db cargo test -p earthly-music-server \
//!     --test search_text_parity -- --ignored

use earthly_music_server::searchdata::{encode, prepare_stored};
use sqlx::{Connection, SqliteConnection};

#[tokio::test]
#[ignore = "needs EARTHLY_PARITY_DB, a copy of a database the Go server wrote"]
async fn rust_normalization_matches_every_text_the_go_server_stored() {
    let path = std::env::var("EARTHLY_PARITY_DB").expect("EARTHLY_PARITY_DB");
    let mut connection = SqliteConnection::connect(&format!("sqlite://{path}?mode=ro"))
        .await
        .expect("open copy");
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT texts.kind, texts.id, sources.name, texts.text_json
         FROM library_search_texts texts
         JOIN library_search_sources sources USING (kind, id)",
    )
    .fetch_all(&mut connection)
    .await
    .expect("rows");
    assert!(!rows.is_empty(), "the database has no Library Search texts");

    let mismatches: Vec<String> = rows
        .iter()
        .filter_map(|(kind, id, name, stored)| {
            let computed = encode(&prepare_stored(name, kind));
            (&computed != stored)
                .then(|| format!("{kind} {id}:\n  go   {stored}\n  rust {computed}"))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} texts differ:\n{}",
        mismatches.len(),
        rows.len(),
        mismatches.join("\n")
    );
    println!(
        "{} Library Search texts match the Go server byte for byte",
        rows.len()
    );
}
