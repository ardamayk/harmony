//! Query strings read like Go's `r.URL.Query()`: `Get` is the first value
//! (serde_urlencoded keeps the last), repeated keys stay countable, and a
//! pair Go cannot parse (a `;`, or a bad percent escape) is left out.

use axum::extract::{FromRequestParts, RawQuery};
use axum::http::request::Parts;
use std::convert::Infallible;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoQuery(Vec<(String, String)>);

impl GoQuery {
    pub fn parse(raw: &str) -> Self {
        Self(
            raw.split('&')
                .filter(|pair| !pair.is_empty() && !pair.contains(';'))
                .filter_map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    Some((unescape(key)?, unescape(value)?))
                })
                .collect(),
        )
    }

    /// The first value for `key`, or "" (Go's `Values.Get`).
    pub fn get(&self, key: &str) -> &str {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    }

    /// Every value for `key`, `None` when the key is absent.
    pub fn values(&self, key: &str) -> Option<Vec<&str>> {
        let values: Vec<&str> = self
            .0
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .collect();
        (!values.is_empty()).then_some(values)
    }
}

/// Go's `url.QueryUnescape`: `+` is a space and every `%` needs two hex
/// digits; invalid UTF-8 is kept as replacement characters.
fn unescape(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = text.get(index + 1..index + 3)?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    Some(String::from_utf8_lossy(&decoded).into_owned())
}

impl<S: Send + Sync> FromRequestParts<S> for GoQuery {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let RawQuery(raw) = RawQuery::from_request_parts(parts, state).await?;
        Ok(Self::parse(raw.as_deref().unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_takes_the_first_value_and_values_counts_repeats() {
        let query = GoQuery::parse("q=a+b&all=true&all=false&q=c");
        assert_eq!(query.get("q"), "a b");
        assert_eq!(query.values("all"), Some(vec!["true", "false"]));
        assert_eq!(query.values("missing"), None);
        assert_eq!(query.get("missing"), "");
    }

    #[test]
    fn pairs_go_cannot_parse_are_left_out() {
        let query = GoQuery::parse("a=%zz&b=1;c=2&d=%E2%82%AC&flag");
        assert_eq!(query.get("a"), "");
        assert_eq!(query.get("b"), "");
        assert_eq!(query.get("d"), "€");
        assert_eq!(query.values("flag"), Some(vec![""]));
    }
}
