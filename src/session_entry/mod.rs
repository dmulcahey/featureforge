use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::cli::session_entry::{SessionEntryRecordArgs, SessionEntryResolveArgs};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::paths::normalize_identifier_token;

const MAX_MESSAGE_BYTES: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SessionPromptOption {
    pub decision: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SessionPrompt {
    pub question: String,
    pub recommended_option: String,
    pub options: Vec<SessionPromptOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SessionEntryResolveOutput {
    pub outcome: String,
    pub decision_source: String,
    pub session_key: String,
    pub decision_path: String,
    pub policy_source: String,
    pub persisted: bool,
    pub failure_class: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<SessionPrompt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecisionState {
    Enabled,
    Bypassed,
    Missing,
    Malformed,
}

pub fn resolve(
    args: &SessionEntryResolveArgs,
) -> Result<SessionEntryResolveOutput, DiagnosticError> {
    let runtime = SessionEntryRuntime::discover(args.session_key.as_deref())?;
    let message_text = runtime.load_message_text(&args.message_file)?;
    let decision_state = runtime.read_decision_state()?;

    match decision_state {
        DecisionState::Enabled => Ok(runtime.result(
            "enabled",
            "existing_enabled",
            true,
            "",
            "existing_enabled",
            None,
        )),
        DecisionState::Bypassed if message_requests_reentry(&message_text) => {
            runtime.write_decision("enabled")?;
            Ok(runtime.result(
                "enabled",
                "explicit_reentry",
                true,
                "",
                "explicit_reentry",
                None,
            ))
        }
        DecisionState::Bypassed => Ok(runtime.result(
            "bypassed",
            "existing_bypassed",
            true,
            "",
            "existing_bypassed",
            None,
        )),
        DecisionState::Missing => Ok(runtime.result(
            "needs_user_choice",
            "missing",
            false,
            "",
            "missing",
            Some(default_prompt()),
        )),
        DecisionState::Malformed => Ok(runtime.result(
            "needs_user_choice",
            "malformed",
            false,
            "MalformedDecisionState",
            "malformed",
            Some(default_prompt()),
        )),
    }
}

pub fn record(args: &SessionEntryRecordArgs) -> Result<SessionEntryResolveOutput, DiagnosticError> {
    let runtime = SessionEntryRuntime::discover(args.session_key.as_deref())?;
    let decision = match args.decision.as_str() {
        "enabled" | "bypassed" => args.decision.as_str(),
        _ => {
            return Err(DiagnosticError::new(
                FailureClass::InvalidCommandInput,
                "record requires --decision enabled|bypassed.",
            ));
        }
    };
    runtime.write_decision(decision)?;
    Ok(runtime.result(
        decision,
        &format!("existing_{decision}"),
        true,
        "",
        &format!("recorded_{decision}"),
        None,
    ))
}

pub fn write_session_entry_schema(output_dir: &Path) -> Result<(), DiagnosticError> {
    fs::create_dir_all(output_dir).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not create session-entry schema directory {}: {error}",
                output_dir.display()
            ),
        )
    })?;
    let schema =
        serde_json::to_string_pretty(&schema_for!(SessionEntryResolveOutput)).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Could not serialize session-entry resolve schema: {error}"),
            )
        })?;
    fs::write(output_dir.join("session-entry-resolve.schema.json"), schema).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write session-entry resolve schema: {error}"),
        )
    })?;
    Ok(())
}

struct SessionEntryRuntime {
    session_key: String,
    legacy_path: PathBuf,
    canonical_path: PathBuf,
}

impl SessionEntryRuntime {
    fn discover(raw_session_key: Option<&str>) -> Result<Self, DiagnosticError> {
        let session_key = derive_session_key(raw_session_key)?;
        let state_dir = state_dir();
        Ok(Self {
            legacy_path: state_dir
                .join("session-flags")
                .join("using-superpowers")
                .join(&session_key),
            canonical_path: state_dir
                .join("session-entry")
                .join("using-superpowers")
                .join(&session_key),
            session_key,
        })
    }

    fn load_message_text(&self, message_file: &Path) -> Result<String, DiagnosticError> {
        let metadata = fs::metadata(message_file).map_err(|_| {
            DiagnosticError::new(
                FailureClass::InvalidCommandInput,
                "--message-file must point to a readable regular file.",
            )
        })?;
        if !metadata.is_file() {
            return Err(DiagnosticError::new(
                FailureClass::InvalidCommandInput,
                "--message-file must point to a readable regular file.",
            ));
        }
        if metadata.len() > max_message_bytes() {
            return Err(DiagnosticError::new(
                FailureClass::InvalidCommandInput,
                "Message file exceeds the supported maximum size.",
            ));
        }
        fs::read_to_string(message_file).map_err(|_| {
            DiagnosticError::new(
                FailureClass::InvalidCommandInput,
                "--message-file must point to a readable regular file.",
            )
        })
    }

    fn read_decision_state(&self) -> Result<DecisionState, DiagnosticError> {
        if self.canonical_path.exists() {
            return read_decision_file(&self.canonical_path);
        }
        if !self.legacy_path.exists() {
            return Ok(DecisionState::Missing);
        }
        let legacy_state = read_decision_file(&self.legacy_path)?;
        if matches!(
            legacy_state,
            DecisionState::Enabled | DecisionState::Bypassed
        ) {
            self.write_decision(match legacy_state {
                DecisionState::Enabled => "enabled",
                DecisionState::Bypassed => "bypassed",
                _ => unreachable!(),
            })?;
        }
        Ok(legacy_state)
    }

    fn write_decision(&self, decision: &str) -> Result<(), DiagnosticError> {
        if let Some(parent) = self.canonical_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DiagnosticError::new(
                    FailureClass::DecisionWriteFailed,
                    format!(
                        "Could not create the session-entry state directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        let tmp_path = self.canonical_path.with_extension("tmp");
        fs::write(&tmp_path, format!("{decision}\n")).map_err(|error| {
            DiagnosticError::new(
                FailureClass::DecisionWriteFailed,
                format!(
                    "Could not write the session-entry temp file {}: {error}",
                    tmp_path.display()
                ),
            )
        })?;
        fs::rename(&tmp_path, &self.canonical_path).map_err(|error| {
            DiagnosticError::new(
                FailureClass::DecisionWriteFailed,
                format!(
                    "Could not move the session-entry decision file into place {}: {error}",
                    self.canonical_path.display()
                ),
            )
        })?;
        Ok(())
    }

    fn result(
        &self,
        outcome: &str,
        decision_source: &str,
        persisted: bool,
        failure_class: &str,
        reason: &str,
        prompt: Option<SessionPrompt>,
    ) -> SessionEntryResolveOutput {
        SessionEntryResolveOutput {
            outcome: outcome.to_owned(),
            decision_source: decision_source.to_owned(),
            session_key: self.session_key.clone(),
            decision_path: self.canonical_path.to_string_lossy().into_owned(),
            policy_source: String::from("default"),
            persisted,
            failure_class: failure_class.to_owned(),
            reason: reason.to_owned(),
            prompt,
        }
    }
}

fn derive_session_key(raw_session_key: Option<&str>) -> Result<String, DiagnosticError> {
    let candidate = raw_session_key
        .map(str::to_owned)
        .or_else(|| env::var("SUPERPOWERS_SESSION_KEY").ok())
        .or_else(|| env::var("PPID").ok())
        .unwrap_or_else(|| String::from("current"));
    let normalized = normalize_identifier_token(&candidate);
    if normalized.is_empty() {
        return Err(DiagnosticError::new(
            FailureClass::InvalidCommandInput,
            "Session key may not be blank after normalization.",
        ));
    }
    Ok(normalized)
}

fn read_decision_file(path: &Path) -> Result<DecisionState, DiagnosticError> {
    if !path.is_file() {
        return Err(DiagnosticError::new(
            FailureClass::DecisionReadFailed,
            "Could not read the persisted session decision.",
        ));
    }
    let contents = fs::read_to_string(path).map_err(|_| {
        DiagnosticError::new(
            FailureClass::DecisionReadFailed,
            "Could not read the persisted session decision.",
        )
    })?;
    match contents.trim() {
        "enabled" => Ok(DecisionState::Enabled),
        "bypassed" => Ok(DecisionState::Bypassed),
        "" => Ok(DecisionState::Malformed),
        _ => Ok(DecisionState::Malformed),
    }
}

fn default_prompt() -> SessionPrompt {
    SessionPrompt {
        question: String::from("Use Superpowers for this session?"),
        recommended_option: String::from("A"),
        options: vec![
            SessionPromptOption {
                decision: String::from("enabled"),
                label: String::from("Use Superpowers"),
            },
            SessionPromptOption {
                decision: String::from("bypassed"),
                label: String::from("Bypass Superpowers"),
            },
        ],
    }
}

fn message_requests_reentry(message: &str) -> bool {
    let lowered = message.to_lowercase().replace(['\'', '’'], "");
    for clause in lowered.split([',', '.', '!', '?', ';', '\n']) {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        if clause == "superpowers please" {
            return true;
        }
        if clause.contains("do not use superpowers")
            || clause.contains("never use superpowers")
            || clause.contains("use no superpowers")
        {
            continue;
        }
        if clause.contains("use superpowers") || clause.contains("enable superpowers") {
            return true;
        }
    }
    false
}

fn state_dir() -> PathBuf {
    env::var_os("SUPERPOWERS_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".superpowers")))
        .unwrap_or_else(|| PathBuf::from(".superpowers"))
}

fn max_message_bytes() -> u64 {
    env::var("SUPERPOWERS_SESSION_ENTRY_MAX_MESSAGE_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(MAX_MESSAGE_BYTES)
}
