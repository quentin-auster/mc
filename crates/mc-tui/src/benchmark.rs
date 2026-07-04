use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use serde::Deserialize;

use crate::edit::EditStrategy;
use crate::telemetry::{self, TelemetryRecord, TelemetryRecorder};

#[derive(Debug, Deserialize)]
struct Fixture {
    id: String,
    prompt: String,
    repository_setup: Option<Vec<String>>,
    strategies: Vec<String>,
    context_commands: Option<Vec<String>>,
    verification_command: Option<String>,
    expected_edit_bytes: Option<usize>,
}

pub fn run() -> Result<(), String> {
    let fixtures = load_fixtures(Path::new("fixtures").join("benchmarks"))?;
    if fixtures.is_empty() {
        return Err("no benchmark fixtures found under fixtures/benchmarks".to_string());
    }

    let recorder = TelemetryRecorder::new(Path::new(".mc").join("benchmark.jsonl"));
    let mut rows = Vec::new();
    for fixture in fixtures {
        for strategy in &fixture.strategies {
            let started = Instant::now();
            let test_result = run_verification(fixture.verification_command.as_deref());
            let wall_time_ms = started.elapsed().as_millis();
            let record = benchmark_record(&fixture, strategy, wall_time_ms, test_result.clone())?;
            recorder.append(&record)?;
            rows.push(ReportRow {
                task_id: fixture.id.clone(),
                strategy: strategy.clone(),
                tokens: record.total_tokens,
                commands: record.command_count,
                edit_bytes: record.edit_bytes,
                wall_time_ms,
                test_result,
            });
        }
    }

    print_report(&rows);
    println!("wrote benchmark telemetry to {}", recorder.path().display());
    Ok(())
}

fn load_fixtures(dir: impl AsRef<Path>) -> Result<Vec<Fixture>, String> {
    let dir = dir.as_ref();
    let mut fixtures: Vec<Fixture> = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read fixture entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        fixtures.push(
            serde_json::from_str(&content)
                .map_err(|e| format!("failed to parse {}: {e}", path.display()))?,
        );
    }
    fixtures.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(fixtures)
}

fn benchmark_record(
    fixture: &Fixture,
    strategy: &str,
    wall_time_ms: u128,
    test_result: Option<String>,
) -> Result<TelemetryRecord, String> {
    let strategy = EditStrategy::parse(strategy)
        .ok_or_else(|| format!("fixture {} has unknown strategy", fixture.id))?;
    let context = fixture.context_commands.as_ref().map(Vec::len).unwrap_or(0);
    let setup_tokens = fixture
        .repository_setup
        .as_ref()
        .map(|steps| telemetry::estimate_tokens(steps.join("\n").len()))
        .unwrap_or(0);
    let mut record = telemetry::turn_record(
        fixture.id.clone(),
        strategy,
        crate::agent::AgentMode::Normal,
        &fixture.prompt,
        None,
        setup_tokens,
        wall_time_ms,
        context,
        fixture.expected_edit_bytes.unwrap_or(0),
    );
    record.test_result = test_result;
    Ok(record)
}

fn run_verification(command: Option<&str>) -> Option<String> {
    let command = command?;
    let output = Command::new("sh").arg("-c").arg(command).output();
    match output {
        Ok(output) if output.status.success() => Some("pass".to_string()),
        Ok(output) => Some(format!("fail:{}", output.status.code().unwrap_or_default())),
        Err(error) => Some(format!("error:{error}")),
    }
}

struct ReportRow {
    task_id: String,
    strategy: String,
    tokens: usize,
    commands: usize,
    edit_bytes: usize,
    wall_time_ms: u128,
    test_result: Option<String>,
}

fn print_report(rows: &[ReportRow]) {
    println!("| task | strategy | tokens | commands | edit bytes | ms | test |");
    println!("| --- | --- | ---: | ---: | ---: | ---: | --- |");
    for row in rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            row.task_id,
            row.strategy,
            row.tokens,
            row.commands,
            row.edit_bytes,
            row.wall_time_ms,
            row.test_result.as_deref().unwrap_or("not-run")
        );
    }
}
