//! Soulseek Acquisition's domain rules. For now it knows only whether the
//! operator configured slskd.

use crate::config::SoulseekConfig;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SoulseekError {
    #[error("Soulseek Acquisition is not configured on this server")]
    Disabled,
    #[error("Soulseek Acquisition is not implemented yet")]
    NotImplemented,
}

pub struct Soulseek {
    config: Option<SoulseekConfig>,
}

impl Soulseek {
    pub fn new(config: Option<&SoulseekConfig>) -> Self {
        Self {
            config: config.cloned(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    pub fn ensure_enabled(&self) -> Result<&SoulseekConfig, SoulseekError> {
        self.config.as_ref().ok_or(SoulseekError::Disabled)
    }
}
