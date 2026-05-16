use serde_json::Value;

pub fn assert_follow_up_blocker_parity_with_operator(
    operator: &Value,
    follow_up: &Value,
    context: &str,
) {
    if follow_up["action"].as_str() != Some("blocked") {
        return;
    }
    let follow_up_blocking_reason_codes = follow_up
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} blocked follow-up must include blocking_reason_codes metadata: {follow_up:?}"
            )
        });
    let operator_blocking_reason_codes = operator
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} operator route must include blocking_reason_codes metadata for blocked parity checks: {operator:?}"
            )
        });
    assert!(
        !follow_up_blocking_reason_codes.is_empty(),
        "{context} blocked follow-up must keep a non-empty blocker reason-code set",
    );
    assert!(
        !operator_blocking_reason_codes.is_empty(),
        "{context} operator route must keep a non-empty blocker reason-code set for blocked parity checks",
    );
    assert_eq!(
        follow_up["blocking_scope"], operator["blocking_scope"],
        "{context} blocked follow-up must preserve operator blocking scope"
    );
    assert_eq!(
        follow_up["blocking_task"], operator["blocking_task"],
        "{context} blocked follow-up must preserve operator blocking task"
    );
    assert_eq!(
        follow_up["blocking_reason_codes"], operator["blocking_reason_codes"],
        "{context} blocked follow-up must preserve operator blocker reason-code set"
    );
    if !follow_up["blocking_step"].is_null() || !operator["blocking_step"].is_null() {
        assert_eq!(
            follow_up["blocking_step"], operator["blocking_step"],
            "{context} blocked follow-up must preserve operator blocking step"
        );
    }

    assert_authoritative_next_action_is_not_display_command(operator, follow_up, context);
}

fn assert_authoritative_next_action_is_not_display_command(
    operator: &Value,
    follow_up: &Value,
    context: &str,
) {
    let Some(authoritative_next_action) = follow_up
        .get("authoritative_next_action")
        .filter(|value| !value.is_null())
    else {
        return;
    };
    let authoritative_next_action = authoritative_next_action.as_str().unwrap_or_else(|| {
        panic!(
            "{context} authoritative_next_action must be null or an intent string, got {follow_up:?}"
        )
    });
    assert!(
        !looks_like_executable_featureforge_command(authoritative_next_action),
        "{context} authoritative_next_action must not contain argv-shaped FeatureForge text; use typed public argv/template instead: {follow_up:?}"
    );
    if let Some(display_command) = operator.get("recommended_command").and_then(Value::as_str) {
        assert_ne!(
            authoritative_next_action, display_command,
            "{context} authoritative_next_action must not mirror display-only recommended_command"
        );
    }
}

fn looks_like_executable_featureforge_command(value: &str) -> bool {
    let trimmed = value.trim();
    ["featureforge ", "./featureforge ", "bin/featureforge "]
        .iter()
        .any(|prefix| trimmed.strip_prefix(prefix).is_some())
}
