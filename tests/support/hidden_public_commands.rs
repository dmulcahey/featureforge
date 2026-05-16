use featureforge::execution::command_eligibility::hidden_command_or_flag_tokens;

pub const LOW_LEVEL_LATE_STAGE_RECORDER_TOKENS: &[&str] = &[
    "record-branch-closure",
    "record-release-readiness",
    "record-final-review",
    "record-qa",
];

const PUBLIC_FLOW_EXTRA_HIDDEN_COMMAND_OR_FLAG_TOKENS: &[&str] = &[
    "preflight",
    "explain-review-state",
    "FEATUREFORGE_ALLOW_INTERNAL_EXECUTION_FLAGS",
];

pub fn public_flow_hidden_command_or_flag_literals() -> Vec<String> {
    let mut tokens = hidden_command_or_flag_tokens()
        .iter()
        .map(|token| (*token).to_owned())
        .collect::<Vec<_>>();
    tokens.extend(
        LOW_LEVEL_LATE_STAGE_RECORDER_TOKENS
            .iter()
            .map(|token| (*token).to_owned()),
    );
    tokens.extend(
        PUBLIC_FLOW_EXTRA_HIDDEN_COMMAND_OR_FLAG_TOKENS
            .iter()
            .map(|token| (*token).to_owned()),
    );
    tokens.sort();
    tokens.dedup();
    tokens
}
