use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::Serialize;

use crate::contracts::plan::ContractDiagnostic;
use crate::paths::normalize_whitespace;

pub const AMBIGUOUS_PHRASES: &[&str] = &[
    "if needed",
    "as needed",
    "as appropriate",
    "appropriately",
    "if helpful",
    "if useful",
    "where possible",
    "handle edge cases",
    "support similar behavior",
    "clean up related code",
    "or equivalent",
    "use a reasonable default",
    "consider adding",
    "is robust",
    "are robust",
    "etc.",
];

const TASK_FIELD_ORDER: &[&str] = &[
    "Spec Coverage",
    "Goal",
    "Context",
    "Constraints",
    "Done when",
    "Files",
];

const GENERIC_DONE_WHEN_SUBJECTS: &[&str] = &[
    "code",
    "the code",
    "change",
    "the change",
    "changes",
    "the changes",
    "feature",
    "the feature",
    "implementation",
    "the implementation",
    "solution",
    "the solution",
    "task",
    "the task",
];

const GENERIC_DONE_WHEN_STATES: &[&str] = &[
    "complete", "correct", "done", "finished", "ready", "robust", "working",
];

const GENERIC_DONE_WHEN_QUALIFIERS: &[&str] = &[
    " as expected",
    " as intended",
    " correctly",
    " properly",
    " for merge",
    " for review",
    " to merge",
    " to release",
    " to review",
    " to ship",
];

const HEDGED_DONE_WHEN_VERBS: &[&str] = &["handle", "handles", "support", "supports"];

const TERMINAL_CONDITION_MARKERS: &[&str] = &[
    " by ",
    " if ",
    " once ",
    " only when ",
    " unless ",
    " when ",
    " whenever ",
];

const SPEC_CONTEXT_TRIGGER_PHRASES: &[&str] = &[
    "approved artifact",
    "approved statement",
    "approved wording",
    "canonical",
    "exact approved",
    "exact spec",
    "hard-fail reuse",
    "packet-backed",
    "requirement wording",
    "reuse boundary",
    "shared helper",
    "single authoritative",
    "spec choice",
    "spec language",
    "spec wording",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TaskContractFields {
    pub goal: String,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContractParseError {
    pub error_class: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTaskBlock {
    pub number: u32,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStepLine {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskIntentSource<'a> {
    pub number: u32,
    pub goal: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContractValidation {
    pub task_contract_valid: bool,
    pub task_goal_valid: bool,
    pub task_context_sufficient: bool,
    pub task_constraints_valid: bool,
    pub task_done_when_deterministic: bool,
    pub task_self_contained: bool,
    pub diagnostics: Vec<ContractDiagnostic>,
}

impl TaskContractValidation {
    pub fn valid() -> Self {
        Self {
            task_contract_valid: true,
            task_goal_valid: true,
            task_context_sufficient: true,
            task_constraints_valid: true,
            task_done_when_deterministic: true,
            task_self_contained: true,
            diagnostics: Vec::new(),
        }
    }

    fn push(&mut self, code: &str, message: impl Into<String>) {
        self.task_contract_valid = false;
        self.diagnostics.push(ContractDiagnostic {
            code: code.to_owned(),
            message: message.into(),
        });
    }
}

pub fn detect_ambiguous_task_wording(lower_text: &str) -> Option<&'static str> {
    AMBIGUOUS_PHRASES
        .iter()
        .copied()
        .find(|phrase| lower_text.contains(phrase))
}

pub fn parse_task_contract_fields(
    task_number: u32,
    lines: &[&str],
) -> Result<TaskContractFields, TaskContractParseError> {
    if lines.iter().any(|line| is_legacy_task_field(line)) {
        return Err(TaskContractParseError {
            error_class: String::from("LegacyTaskField"),
            message: format!(
                "Task {task_number} uses legacy approved-task fields; use Goal, Context, Constraints, and Done when."
            ),
        });
    }
    enforce_task_field_order(task_number, lines)?;

    let fields = TaskContractFields {
        goal: parse_scalar_field(task_number, lines, "Goal", "TaskMissingGoal")?,
        context: parse_required_bullet_field(task_number, lines, "Context", "TaskMissingContext")?,
        constraints: parse_required_bullet_field(
            task_number,
            lines,
            "Constraints",
            "TaskMissingConstraints",
        )?,
        done_when: parse_required_bullet_field(
            task_number,
            lines,
            "Done when",
            "TaskMissingDoneWhen",
        )?,
    };
    validate_task_body_structure(task_number, lines)?;
    Ok(fields)
}

pub fn split_canonical_task_blocks(
    source: &str,
) -> Result<Vec<CanonicalTaskBlock>, TaskContractParseError> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut tasks = Vec::new();
    let mut index = 0;
    let mut seen_numbers = BTreeSet::new();
    while index < lines.len() {
        let line = lines[index];
        if line.starts_with("### Task ") {
            return Err(malformed_task_heading());
        }
        if !line.starts_with("## Task ") {
            index += 1;
            continue;
        }
        let number = parse_canonical_task_heading_number(line)?;
        if !seen_numbers.insert(number) {
            return Err(TaskContractParseError {
                error_class: String::from("MalformedTaskStructure"),
                message: String::from("Task numbers must be unique within the plan."),
            });
        }

        let mut block = vec![line];
        index += 1;
        while index < lines.len() && !lines[index].starts_with("## ") {
            if lines[index].starts_with("### Task ") {
                return Err(malformed_task_heading());
            }
            block.push(lines[index]);
            index += 1;
        }
        tasks.push(CanonicalTaskBlock {
            number,
            source: block.join("\n"),
        });
    }
    Ok(tasks)
}

pub fn parse_task_step_line(line: &str) -> Result<Option<TaskStepLine>, TaskContractParseError> {
    let trimmed = line.trim();
    let Some(rest) = trimmed
        .strip_prefix("- [ ] **Step ")
        .or_else(|| trimmed.strip_prefix("- [x] **Step "))
    else {
        return Ok(None);
    };
    let (number, text) = rest
        .split_once(": ")
        .ok_or_else(|| TaskContractParseError {
            error_class: String::from("MalformedTaskStructure"),
            message: format!("Malformed step entry: {trimmed}"),
        })?;
    Ok(Some(TaskStepLine {
        number: number.parse::<u32>().map_err(|_| TaskContractParseError {
            error_class: String::from("MalformedTaskStructure"),
            message: format!("Malformed step entry: {trimmed}"),
        })?,
        text: text.trim_end_matches("**").to_owned(),
    }))
}

pub fn validate_task_contract(
    task_number: u32,
    title: &str,
    spec_coverage: &[String],
    fields: &TaskContractFields,
) -> TaskContractValidation {
    let mut validation = TaskContractValidation::valid();

    if normalize_whitespace(&fields.goal).is_empty() {
        validation.task_goal_valid = false;
        validation.task_self_contained = false;
        validation.push(
            "task_missing_goal",
            format!("Task {task_number} is missing Goal."),
        );
    } else {
        let normalized_goal = normalize_whitespace(&fields.goal);
        if normalized_goal.eq_ignore_ascii_case(&normalize_whitespace(title)) {
            validation.task_goal_valid = false;
            validation.task_self_contained = false;
            validation.push(
                "task_not_self_contained",
                format!(
                    "Task {task_number} Goal must describe a concrete outcome beyond the title."
                ),
            );
        }
        if sentence_terminator_count(&normalized_goal) > 1 {
            validation.task_goal_valid = false;
            validation.task_self_contained = false;
            validation.push(
                "task_goal_not_atomic",
                format!("Task {task_number} Goal must be exactly one sentence."),
            );
        }
    }

    if let Some(phrase) = detect_ambiguous_task_wording(&fields.goal.to_ascii_lowercase()) {
        validation.task_goal_valid = false;
        validation.task_self_contained = false;
        validation.push(
            "ambiguous_task_wording",
            format!("Task {task_number} Goal uses ambiguous wording ('{phrase}')."),
        );
    }

    if fields.context.is_empty() {
        validation.task_context_sufficient = false;
        validation.task_self_contained = false;
        validation.push(
            "task_missing_context",
            format!("Task {task_number} is missing Context."),
        );
    } else if fields.context.iter().any(|line| is_filler_context(line)) {
        validation.task_context_sufficient = false;
        validation.task_self_contained = false;
        validation.push(
            "task_not_self_contained",
            format!(
                "Task {task_number} Context contains filler instead of implementation context."
            ),
        );
    }

    if requires_explicit_spec_context(spec_coverage, fields)
        && !context_references_required_spec_ids(spec_coverage, fields)
    {
        validation.task_context_sufficient = false;
        validation.task_self_contained = false;
        validation.push(
            "missing_spec_context",
            format!(
                "Task {task_number} Context must cite the decision, non-goal, or spec detail that constrains this task."
            ),
        );
    }

    if let Some(phrase) = detect_ambiguous_field_wording(&fields.context) {
        validation.task_context_sufficient = false;
        validation.task_self_contained = false;
        validation.push(
            "ambiguous_task_wording",
            format!("Task {task_number} Context uses ambiguous wording ('{phrase}')."),
        );
    }

    if fields.constraints.is_empty() {
        validation.task_constraints_valid = false;
        validation.task_self_contained = false;
        validation.push(
            "task_missing_constraints",
            format!("Task {task_number} is missing Constraints."),
        );
    }

    if let Some(phrase) = detect_ambiguous_field_wording(&fields.constraints) {
        validation.task_constraints_valid = false;
        validation.task_self_contained = false;
        validation.push(
            "ambiguous_task_wording",
            format!("Task {task_number} Constraints use ambiguous wording ('{phrase}')."),
        );
    }

    if fields.done_when.is_empty() {
        validation.task_done_when_deterministic = false;
        validation.task_self_contained = false;
        validation.push(
            "task_missing_done_when",
            format!("Task {task_number} is missing Done when."),
        );
        validation.push(
            "task_empty_done_when",
            format!("Task {task_number} Done when must contain at least one obligation."),
        );
    }

    for bullet in &fields.done_when {
        let normalized = normalize_whitespace(bullet);
        if normalized.is_empty() {
            validation.task_done_when_deterministic = false;
            validation.task_self_contained = false;
            validation.push(
                "task_empty_done_when",
                format!("Task {task_number} contains an empty Done when obligation."),
            );
            continue;
        }
        let lower = normalized.to_ascii_lowercase();
        let ambiguous_phrase = detect_ambiguous_task_wording(&lower);
        if let Some(phrase) = ambiguous_phrase {
            validation.task_done_when_deterministic = false;
            validation.task_self_contained = false;
            validation.push(
                "task_nondeterministic_done_when",
                format!(
                    "Task {task_number} Done when obligation uses ambiguous wording ('{phrase}')."
                ),
            );
        }
        if ambiguous_phrase.is_none() && is_generic_done_when_claim(&lower) {
            validation.task_done_when_deterministic = false;
            validation.task_self_contained = false;
            validation.push(
                "task_nondeterministic_done_when",
                format!(
                    "Task {task_number} Done when obligation is too generic to review deterministically."
                ),
            );
        }
        if ambiguous_phrase.is_none() && is_underspecified_hedged_done_when_claim(&lower) {
            validation.task_done_when_deterministic = false;
            validation.task_self_contained = false;
            validation.push(
                "task_nondeterministic_done_when",
                format!(
                    "Task {task_number} Done when obligation uses hedged wording without an exact condition."
                ),
            );
        }
        if lower == "tests pass" || lower == "all tests pass" {
            validation.task_done_when_deterministic = false;
            validation.task_self_contained = false;
            validation.push(
                "task_nondeterministic_done_when",
                format!(
                    "Task {task_number} Done when must name the relevant test surface when tests are the evidence."
                ),
            );
        }
    }

    validation
}

fn validate_task_body_structure(
    task_number: u32,
    lines: &[&str],
) -> Result<(), TaskContractParseError> {
    let mut section = TaskBodySection::None;
    let mut files_seen = false;
    let mut saw_step = false;
    let mut in_fenced_step_detail = false;
    let mut step_numbers = BTreeSet::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if in_fenced_step_detail {
            if trimmed.starts_with("```") {
                in_fenced_step_detail = false;
            }
            continue;
        }
        if saw_step && trimmed.starts_with("```") {
            in_fenced_step_detail = true;
            continue;
        }
        if is_task_body_comment_marker(trimmed) {
            continue;
        }
        if saw_step && is_runtime_execution_note_projection(trimmed) {
            continue;
        }
        if let Some(step) = parse_task_step_line(trimmed)? {
            if !files_seen {
                return Err(TaskContractParseError {
                    error_class: String::from("MalformedTaskStructure"),
                    message: format!("Task {task_number} steps must appear after a Files block."),
                });
            }
            if !step_numbers.insert(step.number) {
                return Err(TaskContractParseError {
                    error_class: String::from("MalformedTaskStructure"),
                    message: format!("Task {task_number} has duplicate Step {}.", step.number),
                });
            }
            saw_step = true;
            section = TaskBodySection::Steps;
            continue;
        }
        if saw_step {
            return Err(unparsed_task_body_line(task_number, trimmed));
        }
        if is_scalar_task_field_line(trimmed) {
            section = TaskBodySection::None;
            continue;
        }
        match trimmed {
            "**Context:**" => {
                section = TaskBodySection::Context;
                continue;
            }
            "**Constraints:**" => {
                section = TaskBodySection::Constraints;
                continue;
            }
            "**Done when:**" => {
                section = TaskBodySection::DoneWhen;
                continue;
            }
            "**Files:**" => {
                files_seen = true;
                section = TaskBodySection::Files;
                continue;
            }
            _ => {}
        }
        match section {
            TaskBodySection::Context | TaskBodySection::Constraints | TaskBodySection::DoneWhen
                if trimmed.starts_with("- ") =>
            {
                continue;
            }
            TaskBodySection::Files => {
                continue;
            }
            _ => {}
        }
        return Err(unparsed_task_body_line(task_number, trimmed));
    }

    if in_fenced_step_detail {
        return Err(TaskContractParseError {
            error_class: String::from("MalformedTaskStructure"),
            message: format!("Task {task_number} has an unterminated step detail fence."),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskBodySection {
    None,
    Context,
    Constraints,
    DoneWhen,
    Files,
    Steps,
}

fn unparsed_task_body_line(task_number: u32, line: &str) -> TaskContractParseError {
    TaskContractParseError {
        error_class: String::from("MalformedTaskStructure"),
        message: format!("Task {task_number} contains unparsed task body line `{line}`."),
    }
}

fn is_runtime_execution_note_projection(line: &str) -> bool {
    line.starts_with("**Execution Note:** Active - ")
        || line.starts_with("**Execution Note:** Blocked - ")
        || line.starts_with("**Execution Note:** Interrupted - ")
}

fn is_task_body_comment_marker(line: &str) -> bool {
    line.starts_with("<!--") && line.ends_with("-->")
}

fn is_scalar_task_field_line(line: &str) -> bool {
    line == "**Spec Coverage:**"
        || line.starts_with("**Spec Coverage:** ")
        || line == "**Goal:**"
        || line.starts_with("**Goal:** ")
}

fn parse_canonical_task_heading_number(line: &str) -> Result<u32, TaskContractParseError> {
    let heading = line
        .strip_prefix("## Task ")
        .ok_or_else(malformed_task_heading)?;
    let (number, _) = heading
        .split_once(": ")
        .ok_or_else(malformed_task_heading)?;
    number.parse::<u32>().map_err(|_| malformed_task_heading())
}

fn malformed_task_heading() -> TaskContractParseError {
    TaskContractParseError {
        error_class: String::from("MalformedTaskStructure"),
        message: String::from("Task headings must use canonical '## Task N:' form."),
    }
}

pub fn normalized_task_intent(fields: &TaskContractFields) -> String {
    normalize_whitespace(&fields.goal).to_ascii_lowercase()
}

pub fn detect_duplicate_task_intents<'a>(
    tasks: impl IntoIterator<Item = TaskIntentSource<'a>>,
) -> Vec<Vec<u32>> {
    let mut by_intent: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for task in tasks {
        let intent = normalize_whitespace(task.goal).to_ascii_lowercase();
        if !intent.is_empty() {
            by_intent.entry(intent).or_default().push(task.number);
        }
    }
    by_intent
        .into_values()
        .filter(|task_numbers| task_numbers.len() > 1)
        .collect()
}

fn detect_ambiguous_field_wording(lines: &[String]) -> Option<&'static str> {
    lines
        .iter()
        .find_map(|line| detect_ambiguous_task_wording(&line.to_ascii_lowercase()))
}

fn is_underspecified_hedged_done_when_claim(lower_text: &str) -> bool {
    let claim = lower_text.trim_end_matches(['.', '!', ':', ';']);
    if let Some(rest) = strip_hedged_done_when_verb(claim) {
        return !names_terminal_condition(rest);
    }
    GENERIC_DONE_WHEN_SUBJECTS.iter().any(|subject| {
        claim
            .strip_prefix(subject)
            .and_then(|rest| strip_hedged_done_when_verb(rest.trim_start()))
            .is_some_and(|rest| !names_terminal_condition(rest))
    })
}

fn strip_hedged_done_when_verb(text: &str) -> Option<&str> {
    HEDGED_DONE_WHEN_VERBS.iter().find_map(|verb| {
        text.strip_prefix(verb)
            .and_then(|rest| rest.strip_prefix(' '))
    })
}

fn names_terminal_condition(text: &str) -> bool {
    TERMINAL_CONDITION_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
}

fn is_generic_done_when_claim(lower_text: &str) -> bool {
    let claim = lower_text.trim_end_matches(['.', '!', ':', ';']);
    matches!(claim, "complete" | "completed" | "done" | "finished")
        || generic_done_when_predicate(claim)
        || GENERIC_DONE_WHEN_SUBJECTS
            .iter()
            .any(|subject| generic_subject_claim(claim, subject))
}

fn sentence_terminator_count(text: &str) -> usize {
    text.char_indices()
        .filter(|(index, character)| {
            matches!(character, '.' | '!' | '?')
                && text[*index + character.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .count()
}

fn generic_subject_claim(claim: &str, subject: &str) -> bool {
    let Some(rest) = claim.strip_prefix(subject) else {
        return false;
    };
    generic_done_when_predicate(rest.trim_start())
        || matches!(rest, " pass" | " passes")
        || rest
            .strip_prefix(" is ")
            .or_else(|| rest.strip_prefix(" are "))
            .is_some_and(generic_done_when_state)
}

fn generic_done_when_predicate(predicate: &str) -> bool {
    matches!(predicate, "work" | "works")
        || generic_with_qualifier(predicate, "work")
        || generic_with_qualifier(predicate, "works")
}

fn generic_done_when_state(state: &str) -> bool {
    GENERIC_DONE_WHEN_STATES.iter().any(|generic_state| {
        state == *generic_state || generic_with_qualifier(state, generic_state)
    })
}

fn generic_with_qualifier(text: &str, generic_prefix: &str) -> bool {
    text.strip_prefix(generic_prefix)
        .is_some_and(|suffix| GENERIC_DONE_WHEN_QUALIFIERS.contains(&suffix))
}

fn is_legacy_task_field(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("**Task Outcome:**")
        || trimmed.starts_with("**Plan Constraints:**")
        || trimmed.starts_with("**Open Questions:**")
}

fn enforce_task_field_order(
    task_number: u32,
    lines: &[&str],
) -> Result<(), TaskContractParseError> {
    let mut previous: Option<(&str, usize)> = None;
    for field in TASK_FIELD_ORDER {
        let positions = task_field_positions(lines, field);
        if positions.len() > 1 {
            return Err(TaskContractParseError {
                error_class: String::from("DuplicateTaskField"),
                message: format!("Task {task_number} contains duplicate `{field}` fields."),
            });
        }
        let Some(position) = positions.first().copied() else {
            continue;
        };
        if let Some((previous_field, previous_position)) = previous
            && position <= previous_position
        {
            return Err(TaskContractParseError {
                error_class: String::from("TaskFieldOrder"),
                message: format!(
                    "Task {task_number} field `{field}` must appear after `{previous_field}`."
                ),
            });
        }
        previous = Some((field, position));
    }
    Ok(())
}

fn task_field_positions(lines: &[&str], field: &str) -> Vec<usize> {
    let scalar_prefix = format!("**{field}:** ");
    let block_marker = format!("**{field}:**");
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            (*line == block_marker || line.starts_with(&scalar_prefix)).then_some(index)
        })
        .collect()
}

fn parse_scalar_field(
    task_number: u32,
    lines: &[&str],
    field: &str,
    error_class: &str,
) -> Result<String, TaskContractParseError> {
    let prefix = format!("**{field}:** ");
    lines
        .iter()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(ToOwned::to_owned)
        .ok_or_else(|| missing_field(task_number, field, error_class))
}

fn parse_required_bullet_field(
    task_number: u32,
    lines: &[&str],
    field: &str,
    error_class: &str,
) -> Result<Vec<String>, TaskContractParseError> {
    let target = format!("**{field}:**");
    if !lines.contains(&target.as_str()) {
        return Err(missing_field(task_number, field, error_class));
    }
    parse_bullets_after_field(task_number, lines, field)
}

fn parse_bullets_after_field(
    task_number: u32,
    lines: &[&str],
    field: &str,
) -> Result<Vec<String>, TaskContractParseError> {
    let target = format!("**{field}:**");
    let mut collecting = false;
    let mut values = Vec::new();
    for line in lines {
        if *line == target {
            collecting = true;
            continue;
        }
        if collecting && line.starts_with("**") {
            break;
        }
        if collecting {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("- ") {
                values.push(value.to_owned());
                continue;
            }
            return Err(TaskContractParseError {
                error_class: String::from("MalformedTaskContractField"),
                message: format!(
                    "Task {task_number} `{field}` entries must be bullets; found `{trimmed}`."
                ),
            });
        }
    }
    Ok(values)
}

fn missing_field(task_number: u32, field: &str, error_class: &str) -> TaskContractParseError {
    TaskContractParseError {
        error_class: error_class.to_owned(),
        message: format!("Task {task_number} is missing {field}."),
    }
}

fn requires_explicit_spec_context(spec_coverage: &[String], fields: &TaskContractFields) -> bool {
    spec_coverage.iter().any(|id| is_constraining_spec_id(id))
        || task_contract_mentions_spec_sensitive_semantics(fields)
}

fn context_references_required_spec_ids(
    spec_coverage: &[String],
    fields: &TaskContractFields,
) -> bool {
    let constraining_ids_referenced = spec_coverage
        .iter()
        .filter(|id| is_constraining_spec_id(id))
        .all(|id| context_references_spec_id(&fields.context, id));
    let spec_semantics_referenced = !task_contract_mentions_spec_sensitive_semantics(fields)
        || spec_coverage
            .iter()
            .any(|id| context_references_spec_id(&fields.context, id));

    constraining_ids_referenced && spec_semantics_referenced
}

fn is_constraining_spec_id(id: &str) -> bool {
    id.starts_with("DEC-") || id.starts_with("NONGOAL-")
}

fn task_contract_mentions_spec_sensitive_semantics(fields: &TaskContractFields) -> bool {
    let mut text = String::new();
    text.push_str(&fields.goal);
    for value in fields.constraints.iter().chain(fields.done_when.iter()) {
        text.push('\n');
        text.push_str(value);
    }
    let lower_text = normalize_whitespace(&text).to_ascii_lowercase();
    SPEC_CONTEXT_TRIGGER_PHRASES
        .iter()
        .any(|phrase| lower_text.contains(phrase))
}

fn context_references_spec_id(context: &[String], expected_id: &str) -> bool {
    context
        .iter()
        .any(|line| references_spec_id(line, expected_id))
}

fn references_spec_id(line: &str, expected_id: &str) -> bool {
    normalize_whitespace(line)
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .any(|token| token == expected_id)
}

fn is_filler_context(line: &str) -> bool {
    matches!(
        normalize_whitespace(line).to_ascii_lowercase().as_str(),
        "" | "n/a" | "none" | "tbd" | "implementation detail" | "context"
    )
}
