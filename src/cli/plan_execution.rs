use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Args)]
pub struct PlanExecutionCli {
    #[command(subcommand)]
    pub command: PlanExecutionCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlanExecutionCommand {
    #[command(about = "Diagnostic routing status query.")]
    Status(StatusArgs),
    #[command(
        name = "repair-review-state",
        about = "Intent-level review-state repair command."
    )]
    RepairReviewState(StatusArgs),
    #[command(
        name = "close-current-task",
        about = "Intent-level task-closure command."
    )]
    CloseCurrentTask(CloseCurrentTaskArgs),
    #[command(
        name = "advance-late-stage",
        about = "Intent-level late-stage progression command."
    )]
    AdvanceLateStage(AdvanceLateStageArgs),
    #[command(about = "Execution step start recorder.")]
    Begin(BeginArgs),
    #[command(about = "Execution step completion recorder.")]
    Complete(CompleteArgs),
    #[command(about = "Execution task reopen recorder.")]
    Reopen(ReopenArgs),
    #[command(about = "Execution handoff transfer recorder.")]
    Transfer(TransferArgs),
    #[command(
        name = "materialize-projections",
        about = "Render runtime projections without changing runtime truth."
    )]
    MaterializeProjections(MaterializeProjectionsArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    pub external_review_result_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewOutcomeArg {
    Pass,
    Fail,
}

impl ReviewOutcomeArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationOutcomeArg {
    Pass,
    Fail,
    #[value(name = "not-run")]
    NotRun,
}

impl VerificationOutcomeArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotRun => "not-run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum AdvanceLateStageResultArg {
    Ready,
    Blocked,
    Pass,
    Fail,
}

impl AdvanceLateStageResultArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct CloseCurrentTaskArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long = "dispatch-id", hide = true)]
    pub dispatch_id: Option<String>,
    #[arg(long = "review-result", value_enum)]
    pub review_result: ReviewOutcomeArg,
    #[arg(long = "review-summary-file")]
    pub review_summary_file: PathBuf,
    #[arg(long = "verification-result", value_enum)]
    pub verification_result: VerificationOutcomeArg,
    #[arg(long = "verification-summary-file")]
    pub verification_summary_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct AdvanceLateStageArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "dispatch-id", hide = true)]
    pub dispatch_id: Option<String>,
    #[arg(long = "branch-closure-id", hide = true)]
    pub branch_closure_id: Option<String>,
    #[arg(long = "reviewer-source")]
    pub reviewer_source: Option<String>,
    #[arg(long = "reviewer-id")]
    pub reviewer_id: Option<String>,
    #[arg(long, value_enum)]
    pub result: Option<AdvanceLateStageResultArg>,
    #[arg(long = "summary-file")]
    pub summary_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct MaterializeProjectionsArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long, value_enum, default_value = "execution")]
    pub scope: MaterializeProjectionScopeArg,
    #[arg(
        long = "tracked",
        conflicts_with = "state_dir",
        help = "Deprecated alias for --repo-export; requires --confirm-repo-export or FEATUREFORGE_ALLOW_REPO_PROJECTION_EXPORT=1."
    )]
    pub tracked: bool,
    #[arg(
        long = "repo-export",
        conflicts_with = "state_dir",
        help = "Write repo-local human-readable projection export files."
    )]
    pub repo_export: bool,
    #[arg(
        long = "confirm-repo-export",
        help = "Acknowledge that repo-local projection export may create Git-visible files."
    )]
    pub confirm_repo_export: bool,
    #[arg(
        long = "state-dir",
        help = "Write projections only under the runtime state directory. This is the default."
    )]
    pub state_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MaterializeProjectionScopeArg {
    Execution,
    LateStage,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TransferScopeArg {
    Task,
    Branch,
}

impl TransferScopeArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Branch => "branch",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct BeginArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long = "execution-mode")]
    pub execution_mode: Option<ExecutionModeArg>,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
pub struct CompleteArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long)]
    pub source: ExecutionModeArg,
    #[arg(long)]
    pub claim: String,
    #[arg(long = "file")]
    pub files: Vec<String>,
    #[arg(long = "verify-command")]
    pub verify_command: Option<String>,
    #[arg(long = "verify-result")]
    pub verify_result: Option<String>,
    #[arg(long = "manual-verify-summary")]
    pub manual_verify_summary: Option<String>,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
pub struct ReopenArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long)]
    pub source: ExecutionModeArg,
    #[arg(long)]
    pub reason: String,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
pub struct TransferArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long, value_enum)]
    pub scope: Option<TransferScopeArg>,
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long = "repair-task")]
    pub repair_task: Option<u32>,
    #[arg(long = "repair-step")]
    pub repair_step: Option<u32>,
    #[arg(long)]
    pub source: Option<ExecutionModeArg>,
    #[arg(long)]
    pub reason: String,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExecutionModeArg {
    #[value(name = "featureforge:executing-plans")]
    ExecutingPlans,
    #[value(name = "featureforge:subagent-driven-development")]
    SubagentDrivenDevelopment,
}

impl ExecutionModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExecutingPlans => "featureforge:executing-plans",
            Self::SubagentDrivenDevelopment => "featureforge:subagent-driven-development",
        }
    }
}
