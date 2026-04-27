use std::path::PathBuf;

use clap::{Args, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cli::plan_execution::ReviewOutcomeArg;

fn parse_positive_u32(raw: &str) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| String::from("--max-jobs must be a positive integer."))?;
    if value == 0 {
        return Err(String::from("--max-jobs must be a positive integer."));
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDispatchScopeArg {
    Task,
    FinalReview,
}

#[derive(Debug, Clone, Args)]
pub struct RecordReviewDispatchArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long, value_enum)]
    pub scope: ReviewDispatchScopeArg,
    #[arg(long)]
    pub task: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseReadinessOutcomeArg {
    Ready,
    Blocked,
}

impl ReleaseReadinessOutcomeArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct RecordBranchClosureArgs {
    #[arg(long)]
    pub plan: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordReleaseReadinessArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "branch-closure-id")]
    pub branch_closure_id: String,
    #[arg(long, value_enum)]
    pub result: ReleaseReadinessOutcomeArg,
    #[arg(long = "summary-file")]
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordFinalReviewArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "branch-closure-id")]
    pub branch_closure_id: String,
    #[arg(long = "dispatch-id")]
    pub dispatch_id: String,
    #[arg(long = "reviewer-source")]
    pub reviewer_source: String,
    #[arg(long = "reviewer-id")]
    pub reviewer_id: String,
    #[arg(long, value_enum)]
    pub result: ReviewOutcomeArg,
    #[arg(long = "summary-file")]
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordQaArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "result", value_enum)]
    pub result: ReviewOutcomeArg,
    #[arg(long = "summary-file")]
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct GateContractArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub contract: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordContractArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub contract: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct GateEvaluatorArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "evaluation")]
    pub evaluation: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordEvaluationArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "evaluation")]
    pub evaluation: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct GateHandoffArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub handoff: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecordHandoffArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub handoff: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecommendArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "isolated-agents")]
    pub isolated_agents: Option<IsolatedAgentsArg>,
    #[arg(long = "session-intent")]
    pub session_intent: Option<SessionIntentArg>,
    #[arg(long = "workspace-prepared")]
    pub workspace_prepared: Option<WorkspacePreparedArg>,
}

#[derive(Debug, Clone, Args)]
pub struct RebuildEvidenceArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub all: bool,
    #[arg(long = "task")]
    pub tasks: Vec<u32>,
    #[arg(long = "step")]
    pub steps: Vec<String>,
    #[arg(long = "include-open")]
    pub include_open: bool,
    #[arg(long = "skip-manual-fallback")]
    pub skip_manual_fallback: bool,
    #[arg(long = "continue-on-error")]
    pub continue_on_error: bool,
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    #[arg(long = "max-jobs", default_value_t = 1, value_parser = parse_positive_u32)]
    pub max_jobs: u32,
    #[arg(
        long = "no-output",
        help = "Suppress command stream capture while preserving deterministic verification summaries."
    )]
    pub no_output: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionTopologyArg {
    #[value(name = "worktree-backed-parallel")]
    WorktreeBackedParallel,
    #[value(name = "conservative-fallback")]
    ConservativeFallback,
}

impl ExecutionTopologyArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorktreeBackedParallel => "worktree-backed-parallel",
            Self::ConservativeFallback => "conservative-fallback",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct NoteArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long)]
    pub state: NoteStateArg,
    #[arg(long)]
    pub message: String,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum IsolatedAgentsArg {
    Available,
    Unavailable,
}

impl IsolatedAgentsArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SessionIntentArg {
    Stay,
    Separate,
    Unknown,
}

impl SessionIntentArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stay => "stay",
            Self::Separate => "separate",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspacePreparedArg {
    Yes,
    No,
    Unknown,
}

impl WorkspacePreparedArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum NoteStateArg {
    Blocked,
    Interrupted,
}

impl NoteStateArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Interrupted => "interrupted",
        }
    }
}
