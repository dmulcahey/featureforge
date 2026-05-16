use std::cell::Cell;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use serde_json::Value;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::approved_plan_discovery::infer_unique_engineering_approved_plan_path;
use crate::execution::context::load_execution_context_for_migration_parity;
use crate::execution::event_log;
use crate::execution::phase::BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE;
use crate::execution::reducer::{
    EventAuthoritySnapshot, reduce_event_authority_for_migration_parity,
};
use crate::execution::route_plan::route_decision_from_runtime_state_with_authority;
use crate::execution::semantic_identity::semantic_workspace_snapshot;
use crate::execution::state::ExecutionRuntime;
use crate::execution::transitions::AuthoritativeTransitionState;

thread_local! {
    static ROUTE_PARITY_VALIDATION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct RouteParityValidationGuard;

impl Drop for RouteParityValidationGuard {
    fn drop(&mut self) {
        ROUTE_PARITY_VALIDATION_DEPTH.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

fn route_parity_validation_in_progress() -> bool {
    ROUTE_PARITY_VALIDATION_DEPTH.with(|depth| depth.get() > 0)
}

fn enter_route_parity_validation() -> RouteParityValidationGuard {
    ROUTE_PARITY_VALIDATION_DEPTH.with(|depth| {
        depth.set(depth.get() + 1);
    });
    RouteParityValidationGuard
}

#[derive(Debug, Clone, PartialEq)]
enum MigrationRouteParityProjection {
    Routed(Value),
    ProjectionBlocked {
        error_class: String,
        message: String,
    },
}

impl MigrationRouteParityProjection {
    fn blocked(error: JsonFailure) -> Self {
        Self::ProjectionBlocked {
            error_class: error.error_class,
            message: error.message,
        }
    }

    fn describe(&self) -> Value {
        match self {
            Self::Routed(value) => value.clone(),
            Self::ProjectionBlocked {
                error_class,
                message,
            } => serde_json::json!({
                "projection_blocked": {
                    "error_class": error_class,
                    "message": message,
                }
            }),
        }
    }
}

pub(crate) fn ensure_event_log_migrated_from_legacy_state_with_route_parity(
    runtime: &ExecutionRuntime,
    legacy_state_path: &Path,
    plan_hint: Option<&str>,
) -> Result<(), JsonFailure> {
    event_log::ensure_event_log_migrated_from_legacy_state(runtime, legacy_state_path)?;
    validate_legacy_migration_route_parity_for_state_path(runtime, legacy_state_path, plan_hint)
}

pub(crate) fn validate_legacy_migration_route_parity_for_state_path(
    runtime: &ExecutionRuntime,
    state_path: &Path,
    plan_hint: Option<&str>,
) -> Result<(), JsonFailure> {
    if route_parity_validation_in_progress() {
        return Ok(());
    }
    let Some(candidate) = event_log::legacy_migration_parity_candidate_for_state_path(state_path)?
    else {
        return Ok(());
    };
    let _route_parity_guard = enter_route_parity_validation();
    let legacy_projection = migration_route_parity_projection_from_router(
        &candidate.legacy_state,
        runtime,
        &candidate.state_path,
        plan_hint,
    )?;
    let reduced_projection = migration_route_parity_projection_from_router(
        &candidate.reduced_state,
        runtime,
        &candidate.state_path,
        plan_hint,
    )?;
    if legacy_projection != reduced_projection {
        let legacy_projection = legacy_projection.describe();
        let reduced_projection = reduced_projection.describe();
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration route parity mismatch. {BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE}\nlegacy={legacy_projection}\nreduced={reduced_projection}"
            ),
        ));
    }
    event_log::best_effort_record_legacy_migration_route_parity_validated_for_state_path(
        &candidate.state_path,
    );
    Ok(())
}

fn migration_route_parity_projection_from_router(
    state: &Value,
    runtime: &ExecutionRuntime,
    state_path: &Path,
    plan_hint: Option<&str>,
) -> Result<MigrationRouteParityProjection, JsonFailure> {
    let plan_path = migration_route_parity_plan_path(state, runtime, plan_hint).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration route parity requires source_plan_path, a caller plan hint, or exactly one approved plan in the runtime repo. {BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE}"
            ),
        )
    })?;
    let route_decision = event_log::with_in_flight_migration_payload_for_state_path(
        state_path,
        state,
        || -> Result<MigrationRouteParityProjection, JsonFailure> {
            let context =
                match load_execution_context_for_migration_parity(runtime, Path::new(&plan_path)) {
                    Ok(context) => context,
                    Err(error) => return Ok(MigrationRouteParityProjection::blocked(error)),
                };
            let authoritative_state = match AuthoritativeTransitionState::from_reduced_event_payload(
                state_path.to_path_buf(),
                state.clone(),
            ) {
                Ok(state) => state,
                Err(error) => return Ok(MigrationRouteParityProjection::blocked(error)),
            };
            let semantic_workspace = match semantic_workspace_snapshot(&context) {
                Ok(snapshot) => snapshot,
                Err(error) => return Ok(MigrationRouteParityProjection::blocked(error)),
            };
            let runtime_state =
                match reduce_event_authority_for_migration_parity(EventAuthoritySnapshot {
                    context: &context,
                    event_authority_state: Some(&authoritative_state),
                    semantic_workspace,
                }) {
                    Ok(state) => state,
                    Err(error) => return Ok(MigrationRouteParityProjection::blocked(error)),
                };
            Ok(MigrationRouteParityProjection::Routed(
                route_projection_value(route_decision_from_runtime_state_with_authority(
                    &runtime_state,
                    Some(&authoritative_state),
                    false,
                    false,
                )?)?,
            ))
        },
    )?;
    Ok(route_decision)
}

fn route_projection_value(
    route_decision: crate::execution::route_plan::RouteDecision,
) -> Result<Value, JsonFailure> {
    let mut projection = serde_json::Map::new();
    projection.insert(
        String::from("state_kind"),
        Value::String(route_decision.state_kind),
    );
    projection.insert(String::from("phase"), Value::String(route_decision.phase));
    projection.insert(
        String::from("phase_detail"),
        Value::String(route_decision.phase_detail),
    );
    projection.insert(
        String::from("review_state_status"),
        Value::String(route_decision.review_state_status),
    );
    projection.insert(
        String::from("next_action"),
        Value::String(route_decision.next_action),
    );
    if let Some(command) = route_decision.recommended_command {
        projection.insert(String::from("recommended_command"), Value::String(command));
    }
    if let Some(required_follow_up) = route_decision.required_follow_up {
        projection.insert(
            String::from("required_follow_up"),
            Value::String(required_follow_up),
        );
    }
    if let Some(next_public_action) = route_decision.next_public_action {
        projection.insert(
            String::from("next_public_action"),
            serde_json::to_value(next_public_action).map_err(|error| {
                JsonFailure::new(
                    FailureClass::BlockedRuntimeBug,
                    format!(
                        "blocked_runtime_bug: event-log migration route parity could not serialize next_public_action: {error}. {BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE}"
                    ),
                )
            })?,
        );
    }
    projection.insert(
        String::from("blocking_reason_codes"),
        serde_json::to_value(route_decision.blocking_reason_codes).map_err(|error| {
            JsonFailure::new(
                FailureClass::BlockedRuntimeBug,
                format!(
                    "blocked_runtime_bug: event-log migration route parity could not serialize blocking_reason_codes: {error}. {BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE}"
                ),
            )
        })?,
    );
    Ok(Value::Object(projection))
}

fn migration_route_parity_plan_path(
    state: &Value,
    runtime: &ExecutionRuntime,
    plan_hint: Option<&str>,
) -> Option<String> {
    json_string(state, "source_plan_path")
        .or_else(|| plan_hint.map(str::to_owned))
        .or_else(|| infer_unique_engineering_approved_plan_path(runtime))
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_hint_is_used_when_legacy_state_has_no_source_plan_path() {
        let runtime = ExecutionRuntime {
            repo_root: PathBuf::from("/tmp/featureforge-repo"),
            git_dir: PathBuf::from("/tmp/featureforge-repo/.git"),
            state_dir: PathBuf::from("/tmp/featureforge-state"),
            repo_slug: String::from("repo"),
            branch_name: String::from("feature"),
            safe_branch: String::from("feature"),
        };
        assert_eq!(
            migration_route_parity_plan_path(
                &serde_json::json!({"schema_version": 1}),
                &runtime,
                Some("docs/featureforge/plans/example.md"),
            )
            .as_deref(),
            Some("docs/featureforge/plans/example.md"),
        );
    }

    #[test]
    fn missing_route_parity_plan_path_reports_stop_guidance() {
        let runtime = ExecutionRuntime {
            repo_root: PathBuf::from("/tmp/featureforge-repo"),
            git_dir: PathBuf::from("/tmp/featureforge-repo/.git"),
            state_dir: PathBuf::from("/tmp/featureforge-state"),
            repo_slug: String::from("repo"),
            branch_name: String::from("feature"),
            safe_branch: String::from("feature"),
        };
        let error = migration_route_parity_projection_from_router(
            &serde_json::json!({"schema_version": 1}),
            &runtime,
            Path::new("/tmp/featureforge-state/state.json"),
            None,
        )
        .expect_err("missing plan path should fail closed as a runtime diagnostic");
        assert_eq!(
            error.error_class,
            FailureClass::BlockedRuntimeBug.as_str(),
            "missing route parity plan path should classify as BlockedRuntimeBug"
        );
        assert!(
            error
                .message
                .contains(BLOCKED_RUNTIME_BUG_STOP_REPORT_GUIDANCE),
            "missing route parity plan path should tell callers to stop/report, got {}",
            error.message
        );
    }
}
