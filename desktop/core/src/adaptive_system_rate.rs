use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

pub trait PipeWireRateAdapter: Send + Sync {
    fn forced_rate_hz(&self) -> Result<Option<u32>, String>;
    fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String>;
}

pub trait AdaptiveCleanupMarker: Send + Sync {
    fn is_required(&self) -> Result<bool, String>;
    fn mark_required(&self) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

pub struct FileAdaptiveCleanupMarker {
    path: PathBuf,
}

impl FileAdaptiveCleanupMarker {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl AdaptiveCleanupMarker for FileAdaptiveCleanupMarker {
    fn is_required(&self) -> Result<bool, String> {
        match fs::metadata(&self.path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "Failed to inspect Adaptive System Rate cleanup marker {}: {error}",
                self.path.display()
            )),
        }
    }

    fn mark_required(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create Adaptive System Rate state directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        fs::write(&self.path, b"cleanup-required\n").map_err(|error| {
            format!(
                "Failed to persist Adaptive System Rate cleanup marker {}: {error}",
                self.path.display()
            )
        })
    }

    fn clear(&self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Failed to clear Adaptive System Rate cleanup marker {}: {error}",
                self.path.display()
            )),
        }
    }
}

struct SessionOnlyCleanupMarker;

impl AdaptiveCleanupMarker for SessionOnlyCleanupMarker {
    fn is_required(&self) -> Result<bool, String> {
        Ok(false)
    }

    fn mark_required(&self) -> Result<(), String> {
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct CommandPipeWireRateAdapter {
    metadata_binary: PathBuf,
    dump_binary: PathBuf,
}

impl CommandPipeWireRateAdapter {
    pub fn new() -> Self {
        Self::with_binaries(PathBuf::from("pw-metadata"), PathBuf::from("pw-dump"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self::with_binaries(binary.clone(), binary)
    }

    pub fn with_binaries(metadata_binary: PathBuf, dump_binary: PathBuf) -> Self {
        Self {
            metadata_binary,
            dump_binary,
        }
    }

    fn metadata_command(&self) -> Command {
        let mut command = Command::new(&self.metadata_binary);
        command.args(["-n", "settings", "0", "clock.force-rate"]);
        command
    }
}

impl Default for CommandPipeWireRateAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeWireRateAdapter for CommandPipeWireRateAdapter {
    fn forced_rate_hz(&self) -> Result<Option<u32>, String> {
        let output = match Command::new(&self.dump_binary).arg("-N").output() {
            Ok(output) => output,
            Err(error) => return Err(self.command_error("query", &error.to_string(), true)),
        };
        if !output.status.success() {
            return Err(self.status_error("query", &output, true));
        }
        parse_force_rate(&output.stdout)
    }

    fn set_forced_rate_hz(&self, rate_hz: Option<u32>) -> Result<(), String> {
        let rate = rate_hz.unwrap_or(0).to_string();
        let output = self
            .metadata_command()
            .arg(&rate)
            .output()
            .map_err(|error| self.command_error("set", &error.to_string(), false))?;
        if !output.status.success() {
            return Err(self.status_error("set", &output, false));
        }
        Ok(())
    }
}

impl CommandPipeWireRateAdapter {
    fn command_error(&self, operation: &str, detail: &str, is_query: bool) -> String {
        let binary = if is_query {
            &self.dump_binary
        } else {
            &self.metadata_binary
        };
        format!(
            "Failed to {operation} PipeWire clock.force-rate with {}: {detail}",
            binary.display()
        )
    }

    fn status_error(
        &self,
        operation: &str,
        output: &std::process::Output,
        is_query: bool,
    ) -> String {
        let status = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated by signal".to_owned());
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        self.command_error(
            operation,
            &format!("exited with status {status}: {stderr}"),
            is_query,
        )
    }
}

fn parse_force_rate(output: &[u8]) -> Result<Option<u32>, String> {
    let objects: Vec<Value> = serde_json::from_slice(output)
        .map_err(|error| format!("Failed to parse PipeWire metadata: {error}"))?;
    let Some(settings) = objects.iter().find(|object| {
        object
            .pointer("/props/metadata.name")
            .and_then(Value::as_str)
            == Some("settings")
    }) else {
        return Ok(None);
    };
    let Some(value) = settings
        .get("metadata")
        .and_then(Value::as_array)
        .and_then(|metadata| {
            metadata.iter().find_map(|entry| {
                (entry.get("key").and_then(Value::as_str) == Some("clock.force-rate"))
                    .then(|| entry.get("value"))
                    .flatten()
            })
        })
    else {
        return Ok(None);
    };
    let rate_hz = value
        .as_u64()
        .and_then(|rate| u32::try_from(rate).ok())
        .or_else(|| Value::as_str(value).and_then(|rate| rate.parse().ok()))
        .ok_or_else(|| "PipeWire clock.force-rate metadata was invalid.".to_owned())?;
    Ok((rate_hz > 0).then_some(rate_hz))
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveSystemRateState {
    pub is_enabled: bool,
    pub forced_rate_hz: Option<u32>,
}

pub struct AdaptiveSystemRateController {
    adapter: Arc<dyn PipeWireRateAdapter>,
    cleanup_marker: Arc<dyn AdaptiveCleanupMarker>,
    state: AdaptiveSystemRateState,
}

impl AdaptiveSystemRateController {
    pub fn new(adapter: Arc<dyn PipeWireRateAdapter>) -> Self {
        Self::with_cleanup_marker(adapter, Arc::new(SessionOnlyCleanupMarker))
    }

    pub fn with_cleanup_marker(
        adapter: Arc<dyn PipeWireRateAdapter>,
        cleanup_marker: Arc<dyn AdaptiveCleanupMarker>,
    ) -> Self {
        Self {
            adapter,
            cleanup_marker,
            state: AdaptiveSystemRateState::default(),
        }
    }

    pub fn recover_startup(adapter: Arc<dyn PipeWireRateAdapter>) -> Result<Self, String> {
        if adapter.forced_rate_hz()?.is_some() {
            adapter.set_forced_rate_hz(None)?;
        }
        Ok(Self::new(adapter))
    }

    pub fn recover_startup_if_marked(
        adapter: Arc<dyn PipeWireRateAdapter>,
        cleanup_marker: Arc<dyn AdaptiveCleanupMarker>,
    ) -> Result<Self, String> {
        if cleanup_marker.is_required()? {
            adapter.set_forced_rate_hz(None)?;
            cleanup_marker.clear()?;
        }
        Ok(Self::with_cleanup_marker(adapter, cleanup_marker))
    }

    pub fn state(&self) -> &AdaptiveSystemRateState {
        &self.state
    }

    pub fn enable(&mut self) -> Result<(), String> {
        self.cleanup_marker.mark_required()?;
        self.state.is_enabled = true;
        Ok(())
    }

    pub fn apply_source_sample_rate(&mut self, rate_hz: Option<u32>) -> Result<(), String> {
        if !self.state.is_enabled {
            return Ok(());
        }
        let rate_hz = rate_hz.filter(|rate| *rate > 0);
        self.adapter.set_forced_rate_hz(rate_hz)?;
        self.state.forced_rate_hz = rate_hz;
        Ok(())
    }

    pub fn disable(&mut self) -> Result<(), String> {
        if !self.state.is_enabled && self.state.forced_rate_hz.is_none() {
            return Ok(());
        }
        self.reset_force_rate()?;
        self.cleanup_marker.clear()?;
        self.state = AdaptiveSystemRateState::default();
        Ok(())
    }

    pub fn reset_for_recovery(&mut self) -> Result<(), String> {
        if !self.state.is_enabled && self.state.forced_rate_hz.is_none() {
            return Ok(());
        }
        self.reset_force_rate()
    }

    fn reset_force_rate(&mut self) -> Result<(), String> {
        self.adapter.set_forced_rate_hz(None)?;
        self.state.forced_rate_hz = None;
        Ok(())
    }
}

impl Drop for AdaptiveSystemRateController {
    fn drop(&mut self) {
        if self.state.is_enabled || self.state.forced_rate_hz.is_some() {
            match self.adapter.set_forced_rate_hz(None) {
                Ok(()) => {
                    if let Err(error) = self.cleanup_marker.clear() {
                        eprintln!("Adaptive System Rate teardown marker cleanup failed: {error}");
                    }
                }
                Err(error) => eprintln!("Adaptive System Rate teardown failed: {error}"),
            }
        }
    }
}
