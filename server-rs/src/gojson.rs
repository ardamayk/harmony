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

/// [`float`] for an optional value; `None` is `null`.
pub fn optional_float<S: Serializer>(
    value: &Option<f64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => float(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// A SQLite DATETIME (`2026-09-06 15:47:56`, optionally with fractional
/// seconds or a zone) as Go writes the `time.Time` the driver parsed from
/// it: RFC 3339 with trailing fractional zeros dropped, UTC as `Z`.
pub fn time(stored: &str) -> String {
    let stored = stored.trim();
    let (date_time, zone) = split_zone(stored);
    let date_time = date_time.replacen(' ', "T", 1);
    let date_time = match date_time.split_once('.') {
        Some((whole, fraction)) => {
            let fraction = fraction.trim_end_matches('0');
            if fraction.is_empty() {
                whole.to_owned()
            } else {
                format!("{whole}.{fraction}")
            }
        }
        None => date_time,
    };
    format!("{date_time}{zone}")
}

fn split_zone(value: &str) -> (&str, String) {
    if let Some(rest) = value.strip_suffix('Z') {
        return (rest, "Z".to_owned());
    }
    // A numeric offset after the time part, e.g. +03:00 or -0500.
    if let Some(index) = value.rfind(['+', '-'])
        && index > 10
    {
        let offset = &value[index..];
        let offset = if offset.len() == 5 && !offset.contains(':') {
            format!("{}:{}", &offset[..3], &offset[3..])
        } else {
            offset.to_owned()
        };
        let offset = if offset == "+00:00" {
            "Z".to_owned()
        } else {
            offset
        };
        return (value[..index].trim_end(), offset);
    }
    (value, "Z".to_owned())
}

/// A SQLite error as the Go server's driver (modernc.org/sqlite) words it,
/// for the few handlers that pass a database error to the client:
/// `"<sqlite3_errstr(code)>: <sqlite3_errmsg> (<code>)"`.
pub fn sqlite_error(error: &sqlx::Error) -> String {
    let Some(database) = error.as_database_error() else {
        return error.to_string();
    };
    let Some(code) = database.code().and_then(|code| code.parse::<i32>().ok()) else {
        return database.message().to_owned();
    };
    // SAFETY: sqlite3_errstr returns a static, NUL-terminated string.
    let description = unsafe { std::ffi::CStr::from_ptr(libsqlite3_sys::sqlite3_errstr(code)) }
        .to_string_lossy()
        .into_owned();
    let message = database.message();
    if message == description {
        format!("{description} ({code})")
    } else {
        format!("{description}: {message} ({code})")
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

    #[test]
    fn sqlite_datetimes_read_like_go_time() {
        assert_eq!(time("2026-09-06 15:47:56"), "2026-09-06T15:47:56Z");
        assert_eq!(
            time("2026-09-06 15:47:56.120000"),
            "2026-09-06T15:47:56.12Z"
        );
        assert_eq!(time("2026-09-06T15:47:56.000Z"), "2026-09-06T15:47:56Z");
        assert_eq!(
            time("2026-09-06 15:47:56+03:00"),
            "2026-09-06T15:47:56+03:00"
        );
        assert_eq!(time("2026-09-06 15:47:56+00:00"), "2026-09-06T15:47:56Z");
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
