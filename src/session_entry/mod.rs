use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::diagnostics::{DiagnosticError, FailureClass};

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
    #[schemars(with = "SessionOutcomeSchemaDoc")]
    pub outcome: String,
    #[schemars(with = "SessionDecisionSourceSchemaDoc")]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SessionOutcomeSchemaDoc {
    Enabled,
    Bypassed,
    NeedsUserChoice,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SessionDecisionSourceSchemaDoc {
    ExistingEnabled,
    ExistingBypassed,
    Missing,
    Malformed,
    ExplicitReentry,
    ExplicitReentryUnpersisted,
    SpawnedSubagentDefault,
    SpawnedSubagentOptIn,
    SpawnedSubagentOptInUnpersisted,
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
