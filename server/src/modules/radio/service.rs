//! Saved-station rules (radio/service.go): stream and link URL validation,
//! Radio Browser imports that update a station instead of duplicating it.

use super::RADIO_BROWSER_SOURCE;
use super::repository::{RadioError, Repository};
use super::stream::validate_stream_url;
use super::types::{List, SearchResult, Station, StationCreate, StationPatch};

pub const MANUAL_SOURCE: &str = "manual";

pub struct Service {
    pub repo: Repository,
}

impl Service {
    pub async fn list(&self, user_id: &str) -> Result<List<Station>, RadioError> {
        let items = self.repo.list(user_id).await?;
        Ok(List {
            total: items.len(),
            items,
        })
    }

    pub async fn get(&self, user_id: &str, station_id: &str) -> Result<Station, RadioError> {
        self.repo.get(user_id, station_id).await
    }

    pub async fn create(
        &self,
        user_id: &str,
        mut input: StationCreate,
    ) -> Result<Station, RadioError> {
        for field in [
            &mut input.name,
            &mut input.stream_url,
            &mut input.homepage_url,
            &mut input.favicon_url,
            &mut input.country,
            &mut input.language,
            &mut input.codec,
            &mut input.source,
            &mut input.external_id,
        ] {
            *field = field.trim().to_owned();
        }
        validate_station(
            &input.name,
            &input.stream_url,
            &input.homepage_url,
            &input.favicon_url,
        )?;
        if input.source.is_empty() {
            input.source = MANUAL_SOURCE.to_owned();
        }
        let id = self.repo.insert(user_id, &input).await?;
        self.repo.get(user_id, &id).await
    }

    pub async fn import(&self, user_id: &str, result: SearchResult) -> Result<Station, RadioError> {
        let input = StationCreate {
            name: result.name,
            stream_url: result.stream_url,
            homepage_url: result.homepage_url,
            favicon_url: result.favicon_url,
            country: result.country,
            language: result.language,
            tags: result.tags.clone().unwrap_or_default(),
            codec: result.codec,
            bitrate: result.bitrate,
            source: RADIO_BROWSER_SOURCE.to_owned(),
            external_id: result.station_uuid,
            is_favorite: false,
        };
        match self
            .repo
            .get_by_external_id(user_id, &input.source, &input.external_id)
            .await
        {
            Err(RadioError::NotFound) => self.create(user_id, input).await,
            Err(error) => Err(error),
            Ok(existing) => {
                let patch = StationPatch {
                    name: Some(input.name),
                    stream_url: Some(input.stream_url),
                    homepage_url: Some(input.homepage_url),
                    favicon_url: Some(input.favicon_url),
                    country: Some(input.country),
                    language: Some(input.language),
                    tags: result.tags,
                    codec: Some(input.codec),
                    bitrate: Some(input.bitrate),
                    is_favorite: None,
                    position: None,
                };
                self.update(user_id, &existing.id, patch).await
            }
        }
    }

    pub async fn update(
        &self,
        user_id: &str,
        station_id: &str,
        patch: StationPatch,
    ) -> Result<Station, RadioError> {
        let mut current = self.repo.get(user_id, station_id).await?;
        for (target, value) in [
            (&mut current.name, patch.name),
            (&mut current.stream_url, patch.stream_url),
            (&mut current.homepage_url, patch.homepage_url),
            (&mut current.favicon_url, patch.favicon_url),
            (&mut current.country, patch.country),
            (&mut current.language, patch.language),
            (&mut current.codec, patch.codec),
        ] {
            if let Some(value) = value {
                *target = value.trim().to_owned();
            }
        }
        if let Some(tags) = patch.tags {
            current.tags = tags;
        }
        if let Some(bitrate) = patch.bitrate {
            current.bitrate = bitrate;
        }
        if let Some(is_favorite) = patch.is_favorite {
            current.is_favorite = is_favorite;
        }
        if let Some(position) = patch.position {
            current.position = position;
        }
        validate_station(
            &current.name,
            &current.stream_url,
            &current.homepage_url,
            &current.favicon_url,
        )?;
        self.repo.update(user_id, &current).await?;
        self.repo.get(user_id, station_id).await
    }

    pub async fn delete(&self, user_id: &str, station_id: &str) -> Result<(), RadioError> {
        self.repo.delete(user_id, station_id).await
    }
}

fn validate_station(
    name: &str,
    stream_url: &str,
    homepage_url: &str,
    favicon_url: &str,
) -> Result<(), RadioError> {
    let invalid = |message: String| Err(RadioError::Invalid(message));
    if name.is_empty() {
        return invalid("station name is required".to_owned());
    }
    if stream_url.is_empty() {
        return invalid("streamUrl is required".to_owned());
    }
    if let Err(error) = validate_stream_url(stream_url) {
        return invalid(format!("invalid streamUrl: {error}"));
    }
    validate_optional_http_url("homepageUrl", homepage_url)?;
    validate_optional_http_url("faviconUrl", favicon_url)
}

fn validate_optional_http_url(field: &str, raw: &str) -> Result<(), RadioError> {
    if raw.is_empty() {
        return Ok(());
    }
    let absolute =
        || RadioError::Invalid(format!("invalid {field}: must be an absolute HTTP(S) URI"));
    match url::Url::parse(raw) {
        Ok(parsed)
            if matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some_and(|host| !host.is_empty()) =>
        {
            Ok(())
        }
        Ok(_) | Err(url::ParseError::RelativeUrlWithoutBase) => Err(absolute()),
        Err(error) => Err(RadioError::Invalid(format!("invalid {field}: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_links_must_be_absolute_http() {
        assert!(validate_optional_http_url("homepageUrl", "").is_ok());
        assert!(validate_optional_http_url("homepageUrl", "https://radio.example").is_ok());
        for raw in ["radio.example", "ftp://radio.example", "mailto:a@b.c"] {
            let error = validate_optional_http_url("homepageUrl", raw)
                .expect_err(raw)
                .to_string();
            assert_eq!(
                error, "invalid homepageUrl: must be an absolute HTTP(S) URI",
                "{raw}"
            );
        }
    }

    #[test]
    fn stations_need_a_name_and_a_public_stream() {
        let message = |name: &str, stream: &str| {
            validate_station(name, stream, "", "")
                .expect_err("invalid")
                .to_string()
        };
        assert_eq!(message("", "https://a.example"), "station name is required");
        assert_eq!(message("x", ""), "streamUrl is required");
        assert_eq!(
            message("x", "http://10.1.1.1/s"),
            "invalid streamUrl: unsafe stream URL: blocked IP"
        );
    }
}
