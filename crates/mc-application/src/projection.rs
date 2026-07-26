use std::collections::{BTreeMap, BTreeSet};

use mc_domain::{RunEvent, RunId, Sequence};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use tracing::{info, instrument};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: RunId,
    pub last_sequence: Option<Sequence>,
    pub phase: Option<String>,
    pub goal: Option<String>,
    pub plan: Vec<PlanItem>,
    pub active_files: BTreeSet<String>,
    pub decisions: BTreeMap<String, Decision>,
    pub hypotheses: BTreeMap<String, Hypothesis>,
    pub validation: Validation,
    pub unresolved_issues: BTreeMap<String, String>,
}

impl RunState {
    #[must_use]
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            last_sequence: None,
            phase: None,
            goal: None,
            plan: Vec::new(),
            active_files: BTreeSet::new(),
            decisions: BTreeMap::new(),
            hypotheses: BTreeMap::new(),
            validation: Validation::default(),
            unresolved_issues: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanItem {
    pub id: String,
    pub text: String,
    pub status: WorkStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub statement: String,
    pub rationale: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub statement: String,
    pub status: HypothesisStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Open,
    Confirmed,
    Rejected,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    pub status: ValidationStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    #[default]
    NotRun,
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("event {event_id} belongs to run {actual}, expected {expected}")]
    MixedRuns {
        event_id: mc_domain::EventId,
        expected: RunId,
        actual: RunId,
    },
    #[error("expected event sequence {expected}, received {actual}")]
    SequenceGap { expected: u64, actual: u64 },
    #[error("event kind {kind} uses unsupported payload version {version}")]
    UnsupportedVersion { kind: String, version: u32 },
    #[error("event kind {kind} has invalid payload: {source}")]
    InvalidPayload {
        kind: String,
        #[source]
        source: serde_json::Error,
    },
}

#[instrument(skip(events), fields(%run_id, event_count = events.len()))]
pub fn project_run(run_id: RunId, events: &[RunEvent]) -> Result<RunState, ProjectionError> {
    let mut state = RunState::new(run_id);
    for event in events {
        apply_event(&mut state, event)?;
    }
    info!(
        last_sequence = state.last_sequence.map(Sequence::get),
        "run state projected"
    );
    Ok(state)
}

pub fn apply_event(state: &mut RunState, event: &RunEvent) -> Result<(), ProjectionError> {
    if event.run_id != state.run_id {
        return Err(ProjectionError::MixedRuns {
            event_id: event.id,
            expected: state.run_id,
            actual: event.run_id,
        });
    }
    let expected = state.last_sequence.map_or(1, |sequence| sequence.get() + 1);
    if event.sequence.get() != expected {
        return Err(ProjectionError::SequenceGap {
            expected,
            actual: event.sequence.get(),
        });
    }

    let kind = event.payload.kind.as_str();
    if is_projection_event(kind) && event.payload.version.get() != 1 {
        return Err(ProjectionError::UnsupportedVersion {
            kind: kind.to_owned(),
            version: event.payload.version.get(),
        });
    }

    match kind {
        "run.phase_set" => state.phase = Some(payload::<TextValue>(event)?.value),
        "run.goal_set" => state.goal = Some(payload::<TextValue>(event)?.value),
        "run.plan_replaced" => state.plan = payload::<PlanPayload>(event)?.items,
        "run.active_file_added" => {
            state
                .active_files
                .insert(payload::<PathPayload>(event)?.path);
        }
        "run.active_file_removed" => {
            state
                .active_files
                .remove(&payload::<PathPayload>(event)?.path);
        }
        "run.decision_recorded" => {
            let payload = payload::<DecisionPayload>(event)?;
            state.decisions.insert(
                payload.id,
                Decision {
                    statement: payload.statement,
                    rationale: payload.rationale,
                },
            );
        }
        "run.hypothesis_recorded" => {
            let payload = payload::<HypothesisPayload>(event)?;
            state.hypotheses.insert(
                payload.id,
                Hypothesis {
                    statement: payload.statement,
                    status: HypothesisStatus::Open,
                },
            );
        }
        "run.hypothesis_resolved" => {
            let payload = payload::<HypothesisResolution>(event)?;
            if let Some(hypothesis) = state.hypotheses.get_mut(&payload.id) {
                hypothesis.status = payload.status;
            }
        }
        "run.validation_updated" => state.validation = payload(event)?,
        "run.issue_opened" => {
            let payload = payload::<IssuePayload>(event)?;
            state.unresolved_issues.insert(payload.id, payload.summary);
        }
        "run.issue_resolved" => {
            state
                .unresolved_issues
                .remove(&payload::<IdPayload>(event)?.id);
        }
        _ => {}
    }
    state.last_sequence = Some(event.sequence);
    Ok(())
}

fn is_projection_event(kind: &str) -> bool {
    matches!(
        kind,
        "run.phase_set"
            | "run.goal_set"
            | "run.plan_replaced"
            | "run.active_file_added"
            | "run.active_file_removed"
            | "run.decision_recorded"
            | "run.hypothesis_recorded"
            | "run.hypothesis_resolved"
            | "run.validation_updated"
            | "run.issue_opened"
            | "run.issue_resolved"
    )
}

fn payload<T: DeserializeOwned>(event: &RunEvent) -> Result<T, ProjectionError> {
    serde_json::from_value(event.payload.data.clone()).map_err(|source| {
        ProjectionError::InvalidPayload {
            kind: event.payload.kind.clone(),
            source,
        }
    })
}

#[derive(Deserialize)]
struct TextValue {
    value: String,
}

#[derive(Deserialize)]
struct PathPayload {
    path: String,
}

#[derive(Deserialize)]
struct PlanPayload {
    items: Vec<PlanItem>,
}

#[derive(Deserialize)]
struct DecisionPayload {
    id: String,
    statement: String,
    rationale: Option<String>,
}

#[derive(Deserialize)]
struct HypothesisPayload {
    id: String,
    statement: String,
}

#[derive(Deserialize)]
struct HypothesisResolution {
    id: String,
    status: HypothesisStatus,
}

#[derive(Deserialize)]
struct IssuePayload {
    id: String,
    summary: String,
}

#[derive(Deserialize)]
struct IdPayload {
    id: String,
}

#[cfg(test)]
mod tests {
    use mc_domain::{EventId, EventPayload, RunEvent, RunId, Sequence, Timestamp, Version};

    use super::{ProjectionError, ValidationStatus, WorkStatus, project_run};

    fn event(run_id: RunId, sequence: u64, kind: &str, data: serde_json::Value) -> RunEvent {
        RunEvent {
            id: EventId::new(),
            run_id,
            sequence: Sequence::new(sequence).unwrap(),
            occurred_at: Timestamp::from_unix_milliseconds(sequence as i64),
            causation_id: None,
            correlation_id: None,
            payload: EventPayload::new(kind, Version::new(1).unwrap(), data).unwrap(),
        }
    }

    #[test]
    fn materializes_run_working_state() {
        let run_id = RunId::new();
        let events = vec![
            event(
                run_id,
                1,
                "run.phase_set",
                serde_json::json!({"value": "implementation"}),
            ),
            event(
                run_id,
                2,
                "run.goal_set",
                serde_json::json!({"value": "ship projection"}),
            ),
            event(
                run_id,
                3,
                "run.plan_replaced",
                serde_json::json!({"items": [{"id": "1", "text": "implement", "status": "in_progress"}]}),
            ),
            event(
                run_id,
                4,
                "run.active_file_added",
                serde_json::json!({"path": "src/lib.rs"}),
            ),
            event(
                run_id,
                5,
                "run.decision_recorded",
                serde_json::json!({"id": "d1", "statement": "use events", "rationale": "replayable"}),
            ),
            event(
                run_id,
                6,
                "run.hypothesis_recorded",
                serde_json::json!({"id": "h1", "statement": "ordering is stable"}),
            ),
            event(
                run_id,
                7,
                "run.validation_updated",
                serde_json::json!({"status": "passed", "summary": "tests pass"}),
            ),
            event(
                run_id,
                8,
                "run.issue_opened",
                serde_json::json!({"id": "i1", "summary": "document API"}),
            ),
        ];

        let state = project_run(run_id, &events).unwrap();
        assert_eq!(state.phase.as_deref(), Some("implementation"));
        assert_eq!(state.goal.as_deref(), Some("ship projection"));
        assert_eq!(state.plan[0].status, WorkStatus::InProgress);
        assert!(state.active_files.contains("src/lib.rs"));
        assert_eq!(
            state.decisions["d1"].rationale.as_deref(),
            Some("replayable")
        );
        assert!(state.hypotheses.contains_key("h1"));
        assert_eq!(state.validation.status, ValidationStatus::Passed);
        assert_eq!(state.unresolved_issues["i1"], "document API");
    }

    #[test]
    fn rejects_sequence_gaps() {
        let run_id = RunId::new();
        let error = project_run(
            run_id,
            &[event(
                run_id,
                2,
                "run.goal_set",
                serde_json::json!({"value": "goal"}),
            )],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ProjectionError::SequenceGap {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn rejects_malformed_known_payloads_but_ignores_unknown_events() {
        let run_id = RunId::new();
        let unknown = event(run_id, 1, "tool.completed", serde_json::json!({}));
        let malformed = event(
            run_id,
            2,
            "run.goal_set",
            serde_json::json!({"wrong": true}),
        );

        let error = project_run(run_id, &[unknown, malformed]).unwrap_err();
        assert!(matches!(error, ProjectionError::InvalidPayload { .. }));
    }
}
