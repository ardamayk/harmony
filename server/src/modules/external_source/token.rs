//! External Track ids: the page URL and extractor this server's own search
//! produced, HMAC-signed with a per-process key. Only a signed id is ever
//! resolved, so a client cannot make the server run yt-dlp on a URL of its
//! choosing. Ids die with the process.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Longer ids are not issued, so a longer one is refused unread.
pub const MAX_ID_LENGTH: usize = 2048;
const INVALID: &str = "invalid External Track id";

/// What an External Track id stands for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackRef {
    /// The page yt-dlp resolves.
    #[serde(rename = "u")]
    pub url: String,
    /// The extractor yt-dlp is pinned to (`--use-extractors`).
    #[serde(rename = "e")]
    pub extractor: String,
}

pub struct TokenSigner {
    key: [u8; 32],
}

impl TokenSigner {
    pub fn new() -> Self {
        let mut key = [0_u8; 32];
        // Two v4 UUIDs are 32 bytes of the OS CSPRNG through the uuid crate.
        for chunk in key.chunks_mut(16) {
            chunk.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..chunk.len()]);
        }
        Self { key }
    }

    fn mac(&self) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts any key length")
    }

    /// `base64url(JSON) "." base64url(HMAC-SHA256)`; `None` when it would
    /// be longer than [`MAX_ID_LENGTH`].
    pub fn sign(&self, track: &TrackRef) -> Option<String> {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(track).expect("track encodes"));
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        let id = format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        );
        (id.len() <= MAX_ID_LENGTH).then_some(id)
    }

    pub fn verify(&self, id: &str) -> Result<TrackRef, String> {
        if id.len() > MAX_ID_LENGTH
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(INVALID.to_owned());
        }
        let (payload, signature) = id.split_once('.').ok_or(INVALID)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| INVALID)?;
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).map_err(|_| INVALID)?;
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| INVALID)?;
        let track: TrackRef = serde_json::from_slice(&payload).map_err(|_| INVALID)?;
        if !is_extractor_key(&track.extractor) || !is_web_url(&track.url) {
            return Err(INVALID.to_owned());
        }
        Ok(track)
    }
}

impl Default for TokenSigner {
    fn default() -> Self {
        Self::new()
    }
}

/// yt-dlp extractor keys are identifiers such as `Soundcloud` or `Youtube`.
pub fn is_extractor_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_web_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// A short, stable stand-in for an id: what logs name instead of URLs, and
/// what HLS resource tokens are bound to.
pub fn fingerprint(id: &str) -> String {
    hex::encode(&Sha256::digest(id.as_bytes())[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> TrackRef {
        TrackRef {
            url: "https://soundcloud.com/artist/track".to_owned(),
            extractor: "Soundcloud".to_owned(),
        }
    }

    #[test]
    fn ids_round_trip_and_are_url_safe() {
        let signer = TokenSigner::new();
        let id = signer.sign(&track()).expect("id");
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        );
        assert_eq!(signer.verify(&id), Ok(track()));
        assert_eq!(signer.sign(&track()), Some(id));
    }

    #[test]
    fn tampered_foreign_and_malformed_ids_are_refused() {
        let signer = TokenSigner::new();
        let id = signer.sign(&track()).expect("id");
        let (payload, signature) = id.split_once('.').expect("dot");
        let forged =
            URL_SAFE_NO_PAD.encode(br#"{"u":"http://169.254.169.254/latest","e":"Generic"}"#);
        for bad in [
            format!("{forged}.{signature}"),
            format!("{payload}x.{signature}"),
            payload.to_owned(),
            format!("{id}/../x"),
            "x".repeat(MAX_ID_LENGTH + 1),
            String::new(),
        ] {
            assert!(signer.verify(&bad).is_err(), "{bad}");
        }
        assert!(TokenSigner::new().verify(&id).is_err());
    }

    #[test]
    fn signed_payloads_must_still_name_a_web_page_and_an_extractor() {
        let signer = TokenSigner::new();
        for (url, extractor) in [
            ("file:///etc/passwd", "Soundcloud"),
            ("https://soundcloud.com/a", "all,-generic"),
            ("https://soundcloud.com/a", ""),
        ] {
            let id = signer
                .sign(&TrackRef {
                    url: url.to_owned(),
                    extractor: extractor.to_owned(),
                })
                .expect("id");
            assert!(signer.verify(&id).is_err(), "{url} {extractor}");
        }
    }

    #[test]
    fn overlong_ids_are_not_issued() {
        let long = TrackRef {
            url: format!("https://example.com/{}", "a".repeat(MAX_ID_LENGTH)),
            extractor: "Generic".to_owned(),
        };
        assert_eq!(TokenSigner::new().sign(&long), None);
    }

    #[test]
    fn fingerprints_are_short_and_stable() {
        assert_eq!(fingerprint("abc"), fingerprint("abc"));
        assert_ne!(fingerprint("abc"), fingerprint("abd"));
        assert_eq!(fingerprint("abc").len(), 16);
    }
}
