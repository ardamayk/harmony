use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum PlaybackSource {
    #[serde(rename = "track")]
    Track {
        track: TrackSource,
        #[serde(rename = "playbackUrl")]
        playback_url: String,
        #[serde(rename = "queueItemId")]
        queue_item_id: Option<String>,
        #[serde(flatten)]
        _extra: Map<String, Value>,
    },
    #[serde(rename = "radio-station")]
    RadioStation {
        station: RadioSource,
        #[serde(rename = "playbackUrl")]
        playback_url: String,
        #[serde(flatten)]
        _extra: Map<String, Value>,
    },
    #[serde(rename = "catalog-preview")]
    CatalogPreview {
        result: CatalogSource,
        #[serde(rename = "playbackUrl")]
        playback_url: String,
        #[serde(flatten)]
        _extra: Map<String, Value>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackSource {
    id: String,
    duration_ms: Option<f64>,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RadioSource {
    id: String,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogSource {
    station_uuid: String,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

impl PlaybackSource {
    pub(crate) fn parse(source: &Value) -> Result<Self, String> {
        serde_json::from_value(source.clone())
            .map_err(|error| format!("Playback Source is invalid: {error}"))
    }

    pub(crate) fn playback_url(&self) -> Result<&str, String> {
        let url = match self {
            Self::Track { playback_url, .. }
            | Self::RadioStation { playback_url, .. }
            | Self::CatalogPreview { playback_url, .. } => playback_url,
        };
        (!url.is_empty())
            .then_some(url.as_str())
            .ok_or_else(|| "Playback Source URL is missing.".to_owned())
    }

    pub(crate) fn duration_seconds(&self) -> f64 {
        let Self::Track { track, .. } = self else {
            return 0.0;
        };
        track
            .duration_ms
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .map_or(0.0, |duration| duration / 1000.0)
    }

    fn identity(&self) -> &str {
        match self {
            Self::Track {
                track,
                queue_item_id,
                ..
            } => queue_item_id.as_deref().unwrap_or(&track.id),
            Self::RadioStation { station, .. } => &station.id,
            Self::CatalogPreview { result, .. } => &result.station_uuid,
        }
    }
}

#[derive(Default)]
pub(crate) struct PlaybackQueueContext {
    sources: Vec<Value>,
    order: Vec<usize>,
    current_index: Option<usize>,
}

impl PlaybackQueueContext {
    pub(crate) fn sync(
        &mut self,
        sources: Vec<Value>,
        current_index: Option<usize>,
    ) -> Result<(), String> {
        if current_index.is_some_and(|index| index >= sources.len()) {
            return Err("Playback Queue current index is out of range.".to_owned());
        }
        for source in &sources {
            PlaybackSource::parse(source)?;
        }
        self.sources = sources;
        self.current_index = current_index;
        self.reset_order();
        Ok(())
    }

    pub(crate) fn align_to_source(&mut self, source: &Value) -> Result<(), String> {
        let identity = PlaybackSource::parse(source)?;
        let mut current_index = None;
        for (index, candidate) in self.sources.iter().enumerate() {
            if PlaybackSource::parse(candidate)?.identity() == identity.identity() {
                current_index = Some(index);
                break;
            }
        }
        self.current_index = current_index;
        Ok(())
    }

    pub(crate) fn shuffle(&mut self) -> Result<(), String> {
        self.reset_order();
        for index in (1..self.order.len()).rev() {
            let mut random = [0_u8; 8];
            getrandom::fill(&mut random)
                .map_err(|_| "Playback Queue shuffle could not be generated.".to_owned())?;
            self.order
                .swap(index, u64::from_le_bytes(random) as usize % (index + 1));
        }
        self.avoid_sequential_successor();
        Ok(())
    }

    pub(crate) fn reset_order(&mut self) {
        self.order = (0..self.sources.len()).collect();
    }

    pub(crate) fn adjacent_source(
        &self,
        direction: QueueDirection,
        should_wrap: bool,
    ) -> Option<Value> {
        let current = self.current_index?;
        let position = self.order.iter().position(|index| *index == current)?;
        let next_position = match direction {
            QueueDirection::Previous if position > 0 => Some(position - 1),
            QueueDirection::Next if position + 1 < self.order.len() => Some(position + 1),
            QueueDirection::Previous if should_wrap => self.order.len().checked_sub(1),
            QueueDirection::Next if should_wrap && !self.order.is_empty() => Some(0),
            _ => None,
        }?;
        self.sources.get(self.order[next_position]).cloned()
    }

    #[cfg(test)]
    pub(crate) fn sources(&self) -> Vec<Value> {
        self.sources.clone()
    }

    fn avoid_sequential_successor(&mut self) {
        let Some(current) = self.current_index else {
            return;
        };
        let Some(position) = self.order.iter().position(|index| *index == current) else {
            return;
        };
        self.order.rotate_left(position);
        if self.order.len() > 2 && self.order[1] == (current + 1) % self.order.len() {
            self.order.swap(1, 2);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum QueueDirection {
    Previous,
    Next,
}
