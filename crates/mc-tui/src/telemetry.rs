use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::agent::AgentMode;
use crate::edit::EditStrategy;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TelemetryRecord {
    pub schema_version: u8,
    pub timestamp_secs: u64,
    pub task_id: String,
    pub strategy: String,
    pub mode: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub wall_time_ms: u128,
    pub command_count: usize,
    pub edit_bytes: usize,
    pub repair_turns: usize,
    pub test_result: Option<String>,
}

pub struct TelemetryRecorder {
    path: PathBuf,
}

impl TelemetryRecorder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default() -> Self {
        Self::new(Path::new(".mc").join("telemetry.jsonl"))
    }

    pub fn append(&self, record: &TelemetryRecord) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create telemetry directory: {e}"))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("failed to open telemetry file: {e}"))?;
        let json = serde_json::to_string(record)
            .map_err(|e| format!("failed to serialize telemetry: {e}"))?;
        writeln!(file, "{json}").map_err(|e| format!("failed to write telemetry: {e}"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn turn_record(
    task_id: String,
    strategy: EditStrategy,
    mode: AgentMode,
    prompt: &str,
    completion: Option<&str>,
    context_tokens: usize,
    wall_time_ms: u128,
    command_count: usize,
    edit_bytes: usize,
) -> TelemetryRecord {
    let prompt_tokens = estimate_tokens(prompt.len()) + context_tokens;
    let completion_tokens = completion
        .map(|text| estimate_tokens(text.len()))
        .unwrap_or(0);
    TelemetryRecord {
        schema_version: 1,
        timestamp_secs: now_secs(),
        task_id,
        strategy: strategy.to_string(),
        mode: mode.to_string(),
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        wall_time_ms,
        command_count,
        edit_bytes,
        repair_turns: 0,
        test_result: None,
    }
}

pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_tokens_from_bytes() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(8), 2);
    }
}
