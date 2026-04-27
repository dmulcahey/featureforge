use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn parse_positive_u32(raw: &str) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| String::from("--max-jobs must be a positive integer."))?;
    if value == 0 {
        return Err(String::from("--max-jobs must be a positive integer."));
    }
    Ok(value)
}

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

#[derive(Debug, Clone, Args)]
pub struct PlanPathArgs {
    #[arg(long)]
    pub plan: PathBuf,
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

#[derive(Debug, Clone, Args)]
pub struct MaterializeProjectionsArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long, value_enum, default_value = "execution")]
    pub scope: MaterializeProjectionScopeArg,
    #[arg(
        long = "tracked",
        conflicts_with = "state_dir",
        help = "Deprecated alias for repo-local projection export; approved plan and evidence files are not modified."
    )]
    pub tracked: bool,
    #[arg(
        long = "state-dir",
        help = "Write projections only under the runtime state directory."
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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
