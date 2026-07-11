use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ShellCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::command::{ContextAction, ContextCommand};
use crate::token;

const OUTPUT_LIMIT_BYTES: usize = 12_000;
const MAX_LISTED_FILES: usize = 200;

#[derive(Clone, Debug)]
pub struct ContextPacket {
    pub id: usize,
    pub command: String,
    pub paths: Vec<String>,
    pub line_range: Option<(usize, usize)>,
    pub byte_count: usize,
    pub estimated_tokens: usize,
    pub timestamp_secs: u64,
    pub pinned: bool,
    pub output: String,
    pub truncated: bool,
}

#[derive(Default)]
pub struct ContextLedger {
    pub packets: Vec<ContextPacket>,
    next_id: usize,
}

impl ContextLedger {
    pub fn new() -> Self {
        Self {
            packets: Vec::new(),
            next_id: 1,
        }
    }

    pub fn record(
        &mut self,
        command: String,
        paths: Vec<String>,
        line_range: Option<(usize, usize)>,
        output: String,
    ) -> usize {
        let (output, truncated) = truncate_output(output);
        let byte_count = output.len();
        let id = self.next_id;
        self.next_id += 1;
        self.packets.push(ContextPacket {
            id,
            command,
            paths,
            line_range,
            byte_count,
            estimated_tokens: token::estimate_bytes(byte_count),
            timestamp_secs: now_secs(),
            pinned: false,
            output,
            truncated,
        });
        id
    }

    pub fn pin(&mut self, id: usize) -> Result<(), String> {
        let packet = self
            .packets
            .iter_mut()
            .find(|packet| packet.id == id)
            .ok_or_else(|| format!("no context packet #{id}"))?;
        packet.pinned = true;
        Ok(())
    }

    pub fn drop(&mut self, id: usize) -> Result<(), String> {
        let len = self.packets.len();
        self.packets.retain(|packet| packet.id != id);
        if self.packets.len() == len {
            Err(format!("no context packet #{id}"))
        } else {
            Ok(())
        }
    }

    pub fn clear(&mut self) {
        self.packets.retain(|packet| packet.pinned);
    }

    pub fn summary(&self) -> String {
        if self.packets.is_empty() {
            return "context: empty".to_string();
        }

        let tokens = self.estimated_tokens();
        let pinned = self.packets.iter().filter(|packet| packet.pinned).count();
        format!(
            "context: {} packet(s), ~{} tokens, {} pinned",
            self.packets.len(),
            tokens,
            pinned
        )
    }

    pub fn list(&self) -> String {
        if self.packets.is_empty() {
            return "context ledger is empty".to_string();
        }

        self.packets
            .iter()
            .map(|packet| {
                let pin = if packet.pinned { " pinned" } else { "" };
                let range = packet
                    .line_range
                    .map(|(start, end)| format!(" lines {start}-{end}"))
                    .unwrap_or_default();
                let truncated = if packet.truncated { " truncated" } else { "" };
                let paths = if packet.paths.is_empty() {
                    "no paths".to_string()
                } else {
                    packet.paths.join(",")
                };
                let output_lines = packet.output.lines().count();
                format!(
                    "#{}{} ~{} tokens {}{}; {} bytes; {} line(s); {}; ts {}{}",
                    packet.id,
                    pin,
                    packet.estimated_tokens,
                    packet.command,
                    range,
                    packet.byte_count,
                    output_lines,
                    paths,
                    packet.timestamp_secs,
                    truncated
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn estimated_tokens(&self) -> usize {
        self.packets
            .iter()
            .map(|packet| packet.estimated_tokens)
            .sum()
    }
}

pub fn execute(command: &ContextCommand, ledger: &mut ContextLedger) -> Result<String, String> {
    match command {
        ContextCommand::Rg(pattern) => record_rg(pattern, ledger),
        ContextCommand::Files(filter) => record_files(filter.as_deref(), ledger),
        ContextCommand::Head { path, lines } => record_head_tail(path, *lines, true, ledger),
        ContextCommand::Tail { path, lines } => record_head_tail(path, *lines, false, ledger),
        ContextCommand::Wc { path } => record_wc(path, ledger),
        ContextCommand::Sed { path, start, end } => record_sed(path, *start, *end, ledger),
        ContextCommand::Awk { path, pattern } => record_awk(path, pattern, ledger),
        ContextCommand::Ledger(action) => execute_ledger(action, ledger),
    }
}

fn execute_ledger(action: &ContextAction, ledger: &mut ContextLedger) -> Result<String, String> {
    match action {
        ContextAction::List => Ok(ledger.list()),
        ContextAction::Pin(id) => {
            ledger.pin(*id)?;
            Ok(format!("pinned context packet #{id}"))
        }
        ContextAction::Drop(id) => {
            ledger.drop(*id)?;
            Ok(format!("dropped context packet #{id}"))
        }
        ContextAction::Clear => {
            ledger.clear();
            Ok("cleared unpinned context packets".to_string())
        }
    }
}

fn record_rg(pattern: &str, ledger: &mut ContextLedger) -> Result<String, String> {
    let output = ShellCommand::new("rg")
        .arg("--line-number")
        .arg("--color")
        .arg("never")
        .arg("--glob")
        .arg("!target")
        .arg("--glob")
        .arg("!.git")
        .arg(pattern)
        .output()
        .map_err(|e| format!("failed to run rg: {e}"))?;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() && text.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.is_empty() {
            text = format!("no matches for '{pattern}'");
        } else {
            return Err(stderr.trim().to_string());
        }
    }

    let id = ledger.record(format!("/rg {pattern}"), Vec::new(), None, text);
    Ok(format!("recorded context packet #{id} from /rg {pattern}"))
}

fn record_files(filter: Option<&str>, ledger: &mut ContextLedger) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files(Path::new("."), filter, &mut files)?;
    files.sort();
    if files.len() > MAX_LISTED_FILES {
        files.truncate(MAX_LISTED_FILES);
        files.push(format!("... capped at {MAX_LISTED_FILES} files"));
    }

    let id = ledger.record(
        filter
            .map(|f| format!("/files {f}"))
            .unwrap_or_else(|| "/files".to_string()),
        files.clone(),
        None,
        files.join("\n"),
    );
    Ok(format!("recorded context packet #{id} from /files"))
}

fn record_head_tail(
    path: &str,
    lines: usize,
    head: bool,
    ledger: &mut ContextLedger,
) -> Result<String, String> {
    let content = read_workspace_file(path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let selected: Vec<&str> = if head {
        all_lines.iter().take(lines).copied().collect()
    } else {
        all_lines
            .iter()
            .rev()
            .take(lines)
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    };
    let output = selected.join("\n");
    let command = if head { "head" } else { "tail" };
    let id = ledger.record(
        format!("/{command} {path} {lines}"),
        vec![path.to_string()],
        None,
        output,
    );
    Ok(format!(
        "recorded context packet #{id} from /{command} {path}"
    ))
}

fn record_wc(path: &str, ledger: &mut ContextLedger) -> Result<String, String> {
    let content = read_workspace_file(path)?;
    let lines = content.lines().count();
    let words = content.split_whitespace().count();
    let bytes = content.len();
    let output = format!("{lines} {words} {bytes} {path}");
    let id = ledger.record(format!("/wc {path}"), vec![path.to_string()], None, output);
    Ok(format!("recorded context packet #{id} from /wc {path}"))
}

fn record_sed(
    path: &str,
    start: usize,
    end: usize,
    ledger: &mut ContextLedger,
) -> Result<String, String> {
    let content = read_workspace_file(path)?;
    let lines: Vec<&str> = content.lines().collect();
    if start == 0 || end < start || end > lines.len() {
        return Err(format!("invalid line range {start}-{end} for {path}"));
    }
    let output = lines[start - 1..end].join("\n");
    let id = ledger.record(
        format!("/sed {path} {start} {end}"),
        vec![path.to_string()],
        Some((start, end)),
        output,
    );
    Ok(format!(
        "recorded context packet #{id} from /sed {path} {start} {end}"
    ))
}

fn record_awk(path: &str, pattern: &str, ledger: &mut ContextLedger) -> Result<String, String> {
    let content = read_workspace_file(path)?;
    let output = content
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(pattern))
        .map(|(index, line)| format!("{}:{line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let output = if output.is_empty() {
        format!("no lines in {path} matched '{pattern}'")
    } else {
        output
    };
    let id = ledger.record(
        format!("/awk {path} {pattern}"),
        vec![path.to_string()],
        None,
        output,
    );
    Ok(format!("recorded context packet #{id} from /awk {path}"))
}

fn collect_files(path: &Path, filter: Option<&str>, out: &mut Vec<String>) -> Result<(), String> {
    for entry in
        fs::read_dir(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, filter, out)?;
        } else {
            let display = path
                .strip_prefix(".")
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let display = display.trim_start_matches('/').to_string();
            if filter
                .map(|needle| display.contains(needle))
                .unwrap_or(true)
            {
                out.push(display);
            }
        }
    }
    Ok(())
}

fn read_workspace_file(path: &str) -> Result<String, String> {
    let path = workspace_path(path)?;
    fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn workspace_path(path: &str) -> Result<PathBuf, String> {
    let raw = Path::new(path);
    if raw.is_absolute() {
        return Err("context paths must be relative to the workspace".to_string());
    }
    if raw
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("context paths cannot contain '..'".to_string());
    }
    Ok(raw.to_path_buf())
}

fn truncate_output(output: String) -> (String, bool) {
    if output.len() <= OUTPUT_LIMIT_BYTES {
        return (output, false);
    }

    let mut truncated = output;
    truncated.truncate(OUTPUT_LIMIT_BYTES);
    truncated.push_str("\n... output truncated");
    (truncated, true)
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
    fn ledger_records_and_pins_packets() {
        let mut ledger = ContextLedger::new();
        let id = ledger.record(
            "/files".to_string(),
            vec!["Cargo.toml".to_string()],
            None,
            "Cargo.toml".to_string(),
        );

        assert_eq!(id, 1);
        assert!(ledger.summary().contains("1 packet"));
        ledger.pin(id).unwrap();
        ledger.clear();
        assert_eq!(ledger.packets.len(), 1);
        assert!(ledger.packets[0].pinned);
    }

    #[test]
    fn ledger_drop_removes_packet() {
        let mut ledger = ContextLedger::new();
        let id = ledger.record(
            "/wc Cargo.toml".to_string(),
            vec!["Cargo.toml".to_string()],
            None,
            "1 2 3 Cargo.toml".to_string(),
        );

        ledger.drop(id).unwrap();
        assert!(ledger.packets.is_empty());
    }
}
