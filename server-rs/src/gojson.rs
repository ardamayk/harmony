//! The few places where Go's encoding/json behaves differently from serde
//! and a client or the database can tell.

use crate::respond::ApiError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serializer};

/// Writes a float like Go: whole numbers without a fraction (`22`, not
/// serde_json's `22.0`). Use with `#[serde(serialize_with = "gojson::float")]`.
pub fn float<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// [`float`] for each element of a list.
pub fn floats<S: Serializer>(values: &[f64], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut sequence = serializer.serialize_seq(Some(values.len()))?;
    for value in values {
        sequence.serialize_element(&GoFloat(*value))?;
    }
    sequence.end()
}

struct GoFloat(f64);

impl serde::Serialize for GoFloat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        float(&self.0, serializer)
    }
}

/// Reads a request body the way `json.NewDecoder(body).Decode(&value)`
/// does: the first JSON value counts and anything after it is ignored, and
/// `null` leaves the value at its default. Any failure answers 400 with
/// `message`.
pub fn decode_body<T: DeserializeOwned + Default>(
    body: &[u8],
    message: &str,
) -> Result<T, ApiError> {
    let mut values = serde_json::Deserializer::from_slice(body).into_iter::<serde_json::Value>();
    let value = match values.next() {
        Some(Ok(value)) => value,
        _ => return Err(ApiError::bad_request(message)),
    };
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value).map_err(|_| ApiError::bad_request(message))
}

/// A field where JSON `null` reads like an absent field, as it does for Go
/// struct fields. Use with `#[serde(default, deserialize_with = "gojson::nullable")]`.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Sizes {
        #[serde(serialize_with = "float")]
        rate: f64,
        #[serde(serialize_with = "floats")]
        sizes: Vec<f64>,
    }

    #[test]
    fn whole_floats_have_no_fraction() {
        let json = serde_json::to_string(&Sizes {
            rate: 1.0,
            sizes: vec![22.0, 50.5, 27.25],
        })
        .expect("json");
        assert_eq!(json, r#"{"rate":1,"sizes":[22,50.5,27.25]}"#);
    }

    #[derive(Debug, Default, Deserialize, PartialEq)]
    struct Body {
        #[serde(default, deserialize_with = "nullable")]
        name: String,
    }

    #[test]
    fn decodes_the_first_value_and_treats_null_as_default() {
        assert_eq!(
            decode_body::<Body>(br#"{"name":"a"} trailing"#, "bad").expect("first value"),
            Body { name: "a".into() }
        );
        assert_eq!(
            decode_body::<Body>(b"null", "bad").expect("null"),
            Body::default()
        );
        assert_eq!(
            decode_body::<Body>(br#"{"name":null}"#, "bad").expect("null field"),
            Body::default()
        );
        assert!(decode_body::<Body>(b"", "bad").is_err());
        assert!(decode_body::<Body>(br#"{"name":3}"#, "bad").is_err());
    }
}
