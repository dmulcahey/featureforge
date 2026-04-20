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
/// Runtime struct.
pub struct PlanExecutionCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: PlanExecutionCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum PlanExecutionCommand {
    #[command(about = "Diagnostic routing status query.")]
    /// Runtime enum variant.
    Status(StatusArgs),
    #[command(
        hide = true,
        about = "Compatibility/debug recommendation helper (not part of normal plan execution flow)."
    )]
    /// Runtime enum variant.
    Recommend(RecommendArgs),
    #[command(
        hide = true,
        about = "Compatibility/debug preflight helper (not part of normal plan execution flow)."
    )]
    /// Runtime enum variant.
    Preflight(StatusArgs),
    #[command(name = "internal", hide = true)]
    /// Runtime enum variant.
    Internal(InternalPlanExecutionCli),
    #[command(
        name = "rebuild-evidence",
        hide = true,
        about = "Compatibility/debug projection-regeneration helper (not part of normal plan execution flow)."
    )]
    /// Runtime enum variant.
    RebuildEvidence(RebuildEvidenceArgs),
    #[command(
        name = "gate-contract",
        hide = true,
        about = "Compatibility/debug contract gate (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    GateContract(GateContractArgs),
    #[command(
        name = "record-contract",
        hide = true,
        about = "Compatibility/debug contract recorder (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    RecordContract(RecordContractArgs),
    #[command(
        name = "gate-evaluator",
        hide = true,
        about = "Compatibility/debug evaluator gate (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    GateEvaluator(GateEvaluatorArgs),
    #[command(
        name = "record-evaluation",
        hide = true,
        about = "Compatibility/debug evaluator recorder (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    RecordEvaluation(RecordEvaluationArgs),
    #[command(
        name = "gate-handoff",
        hide = true,
        about = "Compatibility/debug handoff gate (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    GateHandoff(GateHandoffArgs),
    #[command(
        name = "record-handoff",
        hide = true,
        about = "Compatibility/debug handoff recorder (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    RecordHandoff(RecordHandoffArgs),
    #[command(
        name = "gate-review",
        hide = true,
        about = "Compatibility/debug finish-review checkpoint gate (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    GateReview(StatusArgs),
    #[command(
        name = "record-review-dispatch",
        hide = true,
        about = "Compatibility/debug review-dispatch primitive (normal flow uses close-current-task/advance-late-stage)."
    )]
    /// Runtime enum variant.
    RecordReviewDispatch(RecordReviewDispatchArgs),
    #[command(
        name = "repair-review-state",
        about = "Intent-level review-state repair command."
    )]
    /// Runtime enum variant.
    RepairReviewState(StatusArgs),
    #[command(
        name = "explain-review-state",
        hide = true,
        about = "Compatibility/debug review-state explainer (internal diagnostics)."
    )]
    /// Runtime enum variant.
    ExplainReviewState(StatusArgs),
    #[command(
        name = "gate-finish",
        hide = true,
        about = "Compatibility/debug finish-completion gate (internal workflow boundary)."
    )]
    /// Runtime enum variant.
    GateFinish(StatusArgs),
    #[command(
        name = "close-current-task",
        about = "Intent-level task-closure command."
    )]
    /// Runtime enum variant.
    CloseCurrentTask(CloseCurrentTaskArgs),
    #[command(
        name = "record-branch-closure",
        hide = true,
        about = "Compatibility/debug branch-closure primitive (normal flow uses advance-late-stage)."
    )]
    /// Runtime enum variant.
    RecordBranchClosure(RecordBranchClosureArgs),
    #[command(
        name = "record-release-readiness",
        hide = true,
        about = "Compatibility/debug release-readiness primitive (normal flow uses advance-late-stage)."
    )]
    /// Runtime enum variant.
    RecordReleaseReadiness(RecordReleaseReadinessArgs),
    #[command(
        name = "advance-late-stage",
        about = "Intent-level late-stage progression command."
    )]
    /// Runtime enum variant.
    AdvanceLateStage(AdvanceLateStageArgs),
    #[command(
        name = "record-final-review",
        hide = true,
        about = "Compatibility/debug final-review primitive (normal flow uses advance-late-stage)."
    )]
    /// Runtime enum variant.
    RecordFinalReview(RecordFinalReviewArgs),
    #[command(
        name = "record-qa",
        hide = true,
        about = "Compatibility/debug QA primitive (normal flow uses advance-late-stage)."
    )]
    /// Runtime enum variant.
    RecordQa(RecordQaArgs),
    #[command(about = "Execution step start recorder.")]
    /// Runtime enum variant.
    Begin(BeginArgs),
    #[command(about = "Execution interruption/block note recorder.")]
    /// Runtime enum variant.
    Note(NoteArgs),
    #[command(about = "Execution step completion recorder.")]
    /// Runtime enum variant.
    Complete(CompleteArgs),
    #[command(about = "Execution task reopen recorder.")]
    /// Runtime enum variant.
    Reopen(ReopenArgs),
    #[command(about = "Execution handoff transfer recorder.")]
    /// Runtime enum variant.
    Transfer(TransferArgs),
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct InternalPlanExecutionCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: InternalPlanExecutionCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum InternalPlanExecutionCommand {
    #[command(name = "reconcile-review-state")]
    /// Runtime enum variant.
    ReconcileReviewState(StatusArgs),
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct StatusArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    /// Runtime field.
    pub external_review_result_ready: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct PlanPathArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum ReviewDispatchScopeArg {
    /// Runtime enum variant.
    Task,
    /// Runtime enum variant.
    FinalReview,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordReviewDispatchArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long, value_enum)]
    /// Runtime field.
    pub scope: ReviewDispatchScopeArg,
    #[arg(long)]
    /// Runtime field.
    pub task: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum ReviewOutcomeArg {
    /// Runtime enum variant.
    Pass,
    /// Runtime enum variant.
    Fail,
}

impl ReviewOutcomeArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum VerificationOutcomeArg {
    /// Runtime enum variant.
    Pass,
    /// Runtime enum variant.
    Fail,
    #[value(name = "not-run")]
    /// Runtime enum variant.
    NotRun,
}

impl VerificationOutcomeArg {
    #[must_use]
    /// Runtime constant.
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
/// Runtime enum.
pub enum ReleaseReadinessOutcomeArg {
    /// Runtime enum variant.
    Ready,
    /// Runtime enum variant.
    Blocked,
}

impl ReleaseReadinessOutcomeArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum AdvanceLateStageResultArg {
    /// Runtime enum variant.
    Ready,
    /// Runtime enum variant.
    Blocked,
    /// Runtime enum variant.
    Pass,
    /// Runtime enum variant.
    Fail,
}

impl AdvanceLateStageResultArg {
    #[must_use]
    /// Runtime constant.
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
/// Runtime struct.
pub struct CloseCurrentTaskArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long = "dispatch-id", hide = true)]
    /// Runtime field.
    pub dispatch_id: Option<String>,
    #[arg(long = "review-result", value_enum)]
    /// Runtime field.
    pub review_result: ReviewOutcomeArg,
    #[arg(long = "review-summary-file")]
    /// Runtime field.
    pub review_summary_file: PathBuf,
    #[arg(long = "verification-result", value_enum)]
    /// Runtime field.
    pub verification_result: VerificationOutcomeArg,
    #[arg(long = "verification-summary-file")]
    /// Runtime field.
    pub verification_summary_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordBranchClosureArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordReleaseReadinessArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "branch-closure-id")]
    /// Runtime field.
    pub branch_closure_id: String,
    #[arg(long, value_enum)]
    /// Runtime field.
    pub result: ReleaseReadinessOutcomeArg,
    #[arg(long = "summary-file")]
    /// Runtime field.
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct AdvanceLateStageArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "dispatch-id", hide = true)]
    /// Runtime field.
    pub dispatch_id: Option<String>,
    #[arg(long = "branch-closure-id", hide = true)]
    /// Runtime field.
    pub branch_closure_id: Option<String>,
    #[arg(long = "reviewer-source")]
    /// Runtime field.
    pub reviewer_source: Option<String>,
    #[arg(long = "reviewer-id")]
    /// Runtime field.
    pub reviewer_id: Option<String>,
    #[arg(long, value_enum)]
    /// Runtime field.
    pub result: Option<AdvanceLateStageResultArg>,
    #[arg(long = "summary-file")]
    /// Runtime field.
    pub summary_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordFinalReviewArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "branch-closure-id")]
    /// Runtime field.
    pub branch_closure_id: String,
    #[arg(long = "dispatch-id")]
    /// Runtime field.
    pub dispatch_id: String,
    #[arg(long = "reviewer-source")]
    /// Runtime field.
    pub reviewer_source: String,
    #[arg(long = "reviewer-id")]
    /// Runtime field.
    pub reviewer_id: String,
    #[arg(long, value_enum)]
    /// Runtime field.
    pub result: ReviewOutcomeArg,
    #[arg(long = "summary-file")]
    /// Runtime field.
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordQaArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "result", value_enum)]
    /// Runtime field.
    pub result: ReviewOutcomeArg,
    #[arg(long = "summary-file")]
    /// Runtime field.
    pub summary_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct GateContractArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub contract: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordContractArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub contract: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct GateEvaluatorArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "evaluation")]
    /// Runtime field.
    pub evaluation: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordEvaluationArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "evaluation")]
    /// Runtime field.
    pub evaluation: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct GateHandoffArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub handoff: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordHandoffArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub handoff: PathBuf,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecommendArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "isolated-agents")]
    /// Runtime field.
    pub isolated_agents: Option<IsolatedAgentsArg>,
    #[arg(long = "session-intent")]
    /// Runtime field.
    pub session_intent: Option<SessionIntentArg>,
    #[arg(long = "workspace-prepared")]
    /// Runtime field.
    pub workspace_prepared: Option<WorkspacePreparedArg>,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RebuildEvidenceScopeArgs {
    #[arg(long)]
    /// Runtime field.
    pub all: bool,
    #[arg(long = "include-open")]
    /// Runtime field.
    pub include_open: bool,
    #[arg(long = "skip-manual-fallback")]
    /// Runtime field.
    pub skip_manual_fallback: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RebuildEvidenceRunArgs {
    #[arg(long = "continue-on-error")]
    /// Runtime field.
    pub continue_on_error: bool,
    #[arg(long = "dry-run")]
    /// Runtime field.
    pub dry_run: bool,
    #[arg(
        long = "no-output",
        help = "Suppress command stream capture while preserving deterministic verification summaries."
    )]
    /// Runtime field.
    pub no_output: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RebuildEvidenceOutputArgs {
    #[arg(long)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RebuildEvidenceArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "task")]
    /// Runtime field.
    pub tasks: Vec<u32>,
    #[arg(long = "step")]
    /// Runtime field.
    pub steps: Vec<String>,
    #[arg(long = "max-jobs", default_value_t = 1, value_parser = parse_positive_u32)]
    /// Runtime field.
    pub max_jobs: u32,
    #[command(flatten)]
    /// Runtime field.
    pub scope: RebuildEvidenceScopeArgs,
    #[command(flatten)]
    /// Runtime field.
    pub run: RebuildEvidenceRunArgs,
    #[command(flatten)]
    /// Runtime field.
    pub output: RebuildEvidenceOutputArgs,
}

impl RebuildEvidenceArgs {
    #[must_use]
    /// Runtime constant.
    pub const fn all(&self) -> bool {
        self.scope.all
    }

    #[must_use]
    /// Runtime constant.
    pub const fn include_open(&self) -> bool {
        self.scope.include_open
    }

    #[must_use]
    /// Runtime constant.
    pub const fn skip_manual_fallback(&self) -> bool {
        self.scope.skip_manual_fallback
    }

    #[must_use]
    /// Runtime constant.
    pub const fn continue_on_error(&self) -> bool {
        self.run.continue_on_error
    }

    #[must_use]
    /// Runtime constant.
    pub const fn dry_run(&self) -> bool {
        self.run.dry_run
    }

    #[must_use]
    /// Runtime constant.
    pub const fn no_output(&self) -> bool {
        self.run.no_output
    }

    #[must_use]
    /// Runtime constant.
    pub const fn json(&self) -> bool {
        self.output.json
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum ExecutionTopologyArg {
    #[value(name = "worktree-backed-parallel")]
    /// Runtime enum variant.
    WorktreeBackedParallel,
    #[value(name = "conservative-fallback")]
    /// Runtime enum variant.
    ConservativeFallback,
}

impl ExecutionTopologyArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorktreeBackedParallel => "worktree-backed-parallel",
            Self::ConservativeFallback => "conservative-fallback",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "kebab-case")]
/// Runtime enum.
pub enum TransferScopeArg {
    /// Runtime enum variant.
    Task,
    /// Runtime enum variant.
    Branch,
}

impl TransferScopeArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Branch => "branch",
        }
    }
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct BeginArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long)]
    /// Runtime field.
    pub step: u32,
    #[arg(long = "execution-mode")]
    /// Runtime field.
    pub execution_mode: Option<ExecutionModeArg>,
    #[arg(long = "expect-execution-fingerprint")]
    /// Runtime field.
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct NoteArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long)]
    /// Runtime field.
    pub step: u32,
    #[arg(long)]
    /// Runtime field.
    pub state: NoteStateArg,
    #[arg(long)]
    /// Runtime field.
    pub message: String,
    #[arg(long = "expect-execution-fingerprint")]
    /// Runtime field.
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct CompleteArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long)]
    /// Runtime field.
    pub step: u32,
    #[arg(long)]
    /// Runtime field.
    pub source: ExecutionModeArg,
    #[arg(long)]
    /// Runtime field.
    pub claim: String,
    #[arg(long = "file")]
    /// Runtime field.
    pub files: Vec<String>,
    #[arg(long = "verify-command")]
    /// Runtime field.
    pub verify_command: Option<String>,
    #[arg(long = "verify-result")]
    /// Runtime field.
    pub verify_result: Option<String>,
    #[arg(long = "manual-verify-summary")]
    /// Runtime field.
    pub manual_verify_summary: Option<String>,
    #[arg(long = "expect-execution-fingerprint")]
    /// Runtime field.
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct ReopenArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long)]
    /// Runtime field.
    pub step: u32,
    #[arg(long)]
    /// Runtime field.
    pub source: ExecutionModeArg,
    #[arg(long)]
    /// Runtime field.
    pub reason: String,
    #[arg(long = "expect-execution-fingerprint")]
    /// Runtime field.
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct TransferArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long, value_enum)]
    /// Runtime field.
    pub scope: Option<TransferScopeArg>,
    #[arg(long)]
    /// Runtime field.
    pub to: Option<String>,
    #[arg(long = "repair-task")]
    /// Runtime field.
    pub repair_task: Option<u32>,
    #[arg(long = "repair-step")]
    /// Runtime field.
    pub repair_step: Option<u32>,
    #[arg(long)]
    /// Runtime field.
    pub source: Option<ExecutionModeArg>,
    #[arg(long)]
    /// Runtime field.
    pub reason: String,
    #[arg(long = "expect-execution-fingerprint")]
    /// Runtime field.
    pub expect_execution_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum IsolatedAgentsArg {
    /// Runtime enum variant.
    Available,
    /// Runtime enum variant.
    Unavailable,
}

impl IsolatedAgentsArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum SessionIntentArg {
    /// Runtime enum variant.
    Stay,
    /// Runtime enum variant.
    Separate,
    /// Runtime enum variant.
    Unknown,
}

impl SessionIntentArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stay => "stay",
            Self::Separate => "separate",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum WorkspacePreparedArg {
    /// Runtime enum variant.
    Yes,
    /// Runtime enum variant.
    No,
    /// Runtime enum variant.
    Unknown,
}

impl WorkspacePreparedArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum ExecutionModeArg {
    #[value(name = "featureforge:executing-plans")]
    /// Runtime enum variant.
    ExecutingPlans,
    #[value(name = "featureforge:subagent-driven-development")]
    /// Runtime enum variant.
    SubagentDrivenDevelopment,
}

impl ExecutionModeArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutingPlans => "featureforge:executing-plans",
            Self::SubagentDrivenDevelopment => "featureforge:subagent-driven-development",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum NoteStateArg {
    /// Runtime enum variant.
    Blocked,
    /// Runtime enum variant.
    Interrupted,
}

impl NoteStateArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Interrupted => "interrupted",
        }
    }
}
