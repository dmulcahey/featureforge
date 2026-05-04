use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildEvidenceRequest {
    pub plan: PathBuf,
    pub all: bool,
    pub tasks: Vec<u32>,
    pub steps: Vec<(u32, u32)>,
    pub raw_steps: Vec<String>,
    pub include_open: bool,
    pub skip_manual_fallback: bool,
    pub continue_on_error: bool,
    pub dry_run: bool,
    pub max_jobs: u32,
    pub no_output: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildEvidenceCandidate {
    pub task: u32,
    pub step: u32,
    pub order_key: (u32, u32),
    pub target_kind: String,
    pub pre_invalidation_reason: String,
    pub verify_command: Option<String>,
    pub verify_mode: String,
    pub claim: String,
    pub files: Vec<String>,
    pub attempt_number: Option<u32>,
    pub artifact_epoch: Option<String>,
    pub needs_reopen: bool,
}

#[derive(Debug, Clone)]
pub struct CompleteRequest {
    pub task: u32,
    pub step: u32,
    pub source: String,
    pub claim: String,
    pub files: Vec<String>,
    pub verify_command: Option<String>,
    pub verification_summary: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct BeginRequest {
    pub task: u32,
    pub step: u32,
    pub execution_mode: Option<String>,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct NoteRequest {
    pub task: u32,
    pub step: u32,
    pub state: NoteState,
    pub message: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct ReopenRequest {
    pub task: u32,
    pub step: u32,
    pub source: String,
    pub reason: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub reason: String,
    pub mode: TransferRequestMode,
}

#[derive(Debug, Clone)]
pub enum TransferRequestMode {
    RepairStep {
        repair_task: u32,
        repair_step: u32,
        source: String,
        expect_execution_fingerprint: String,
    },
    WorkflowHandoff {
        scope: String,
        to: String,
    },
}

pub fn normalize_begin_request(args: &BeginArgs) -> BeginRequest {
    BeginRequest {
        task: args.task,
        step: args.step,
        execution_mode: args.execution_mode.map(|value| value.as_str().to_owned()),
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    }
}

pub fn normalize_note_request(args: &NoteArgs) -> Result<NoteRequest, JsonFailure> {
    let message = require_normalized_text(
        &args.message,
        FailureClass::InvalidCommandInput,
        "Execution note summaries may not be blank after whitespace normalization.",
    )?;
    if message.chars().count() > 120 {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Execution note summaries may not exceed 120 characters.",
        ));
    }
    let state = match args.state {
        NoteStateArg::Blocked => NoteState::Blocked,
        NoteStateArg::Interrupted => NoteState::Interrupted,
    };

    Ok(NoteRequest {
        task: args.task,
        step: args.step,
        state,
        message,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_complete_request(args: &CompleteArgs) -> Result<CompleteRequest, JsonFailure> {
    let claim = require_normalized_text(
        &args.claim,
        FailureClass::InvalidCommandInput,
        "Completion claims may not be blank after whitespace normalization.",
    )?;
    let verification_summary = match (
        args.verify_command.as_deref(),
        args.verify_result.as_deref(),
        args.manual_verify_summary.as_deref(),
    ) {
        (Some(_), Some(_), Some(_)) | (Some(_), None, _) | (None, Some(_), _) => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "complete accepts exactly one verification mode.",
            ));
        }
        (Some(command), Some(result), None) => {
            let command = require_normalized_text(
                command,
                FailureClass::InvalidCommandInput,
                "Verification commands may not be blank after whitespace normalization.",
            )?;
            let result = require_normalized_text(
                result,
                FailureClass::InvalidCommandInput,
                "Verification results may not be blank after whitespace normalization.",
            )?;
            format!("`{command}` -> {result}")
        }
        (None, None, Some(summary)) => {
            let summary = require_normalized_text(
                summary,
                FailureClass::InvalidCommandInput,
                "Manual verification summaries may not be blank after whitespace normalization.",
            )?;
            format!("Manual inspection only: {summary}")
        }
        (None, None, None) => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "complete requires exactly one verification mode.",
            ));
        }
    };

    Ok(CompleteRequest {
        task: args.task,
        step: args.step,
        source: args.source.as_str().to_owned(),
        claim,
        files: args.files.clone(),
        verify_command: args
            .verify_command
            .as_deref()
            .map(normalize_whitespace)
            .filter(|value| !value.is_empty()),
        verification_summary,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_reopen_request(args: &ReopenArgs) -> Result<ReopenRequest, JsonFailure> {
    Ok(ReopenRequest {
        task: args.task,
        step: args.step,
        source: args.source.as_str().to_owned(),
        reason: require_normalized_text(
            &args.reason,
            FailureClass::InvalidCommandInput,
            "Reopen reasons may not be blank after whitespace normalization.",
        )?,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_transfer_request(args: &TransferArgs) -> Result<TransferRequest, JsonFailure> {
    let reason = require_normalized_text(
        &args.reason,
        FailureClass::InvalidCommandInput,
        "Transfer reasons may not be blank after whitespace normalization.",
    )?;
    let routed_shape_present = args.scope.is_some() || args.to.is_some();
    let legacy_shape_present = args.repair_task.is_some()
        || args.repair_step.is_some()
        || args.source.is_some()
        || args.expect_execution_fingerprint.is_some();

    if routed_shape_present && legacy_shape_present {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer accepts either the routed handoff shape (--scope/--to/--reason) or the legacy repair-step shape (--repair-task/--repair-step/--source/--expect-execution-fingerprint), but not both at once.",
        ));
    }

    if routed_shape_present {
        let scope = args.scope.ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "transfer routed handoff mode requires --scope.",
            )
        })?;
        let to = require_normalized_text(
            args.to.as_deref().unwrap_or_default(),
            FailureClass::InvalidCommandInput,
            "transfer routed handoff mode requires --to.",
        )?;
        return Ok(TransferRequest {
            reason,
            mode: TransferRequestMode::WorkflowHandoff {
                scope: scope.as_str().to_owned(),
                to,
            },
        });
    }

    let repair_task = args.repair_task.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --repair-task.",
        )
    })?;
    let repair_step = args.repair_step.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --repair-step.",
        )
    })?;
    let source = args.source.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --source.",
        )
    })?;
    let expect_execution_fingerprint =
        args.expect_execution_fingerprint.clone().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "transfer legacy repair-step mode requires --expect-execution-fingerprint.",
            )
        })?;

    Ok(TransferRequest {
        reason,
        mode: TransferRequestMode::RepairStep {
            repair_task,
            repair_step,
            source: source.as_str().to_owned(),
            expect_execution_fingerprint,
        },
    })
}

pub fn require_normalized_text(
    value: &str,
    failure_class: FailureClass,
    message: &str,
) -> Result<String, JsonFailure> {
    let normalized = normalize_whitespace(value);
    if normalized.is_empty() {
        return Err(JsonFailure::new(failure_class, message));
    }
    Ok(normalized)
}
