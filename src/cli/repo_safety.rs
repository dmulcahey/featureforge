use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
/// Runtime struct.
pub struct RepoSafetyCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: RepoSafetyCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum RepoSafetyCommand {
    /// Runtime enum variant.
    Check(RepoSafetyCheckArgs),
    /// Runtime enum variant.
    Approve(RepoSafetyApproveArgs),
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RepoSafetyCheckArgs {
    #[arg(long)]
    /// Runtime field.
    pub intent: RepoSafetyIntentArg,
    #[arg(long)]
    /// Runtime field.
    pub stage: String,
    #[arg(long = "task-id")]
    /// Runtime field.
    pub task_id: Option<String>,
    #[arg(long = "path")]
    /// Runtime field.
    pub paths: Vec<String>,
    #[arg(long = "write-target")]
    /// Runtime field.
    pub write_targets: Vec<RepoSafetyWriteTargetArg>,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RepoSafetyApproveArgs {
    #[arg(long)]
    /// Runtime field.
    pub stage: String,
    #[arg(long = "task-id")]
    /// Runtime field.
    pub task_id: Option<String>,
    #[arg(long)]
    /// Runtime field.
    pub reason: String,
    #[arg(long = "path")]
    /// Runtime field.
    pub paths: Vec<String>,
    #[arg(long = "write-target")]
    /// Runtime field.
    pub write_targets: Vec<RepoSafetyWriteTargetArg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum RepoSafetyIntentArg {
    /// Runtime enum variant.
    Read,
    /// Runtime enum variant.
    Write,
}

impl RepoSafetyIntentArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum RepoSafetyWriteTargetArg {
    #[value(name = "spec-artifact-write")]
    /// Runtime enum variant.
    SpecArtifactWrite,
    #[value(name = "plan-artifact-write")]
    /// Runtime enum variant.
    PlanArtifactWrite,
    #[value(name = "approval-header-write")]
    /// Runtime enum variant.
    ApprovalHeaderWrite,
    #[value(name = "execution-task-slice")]
    /// Runtime enum variant.
    ExecutionTaskSlice,
    #[value(name = "release-doc-write")]
    /// Runtime enum variant.
    ReleaseDocWrite,
    #[value(name = "repo-file-write")]
    /// Runtime enum variant.
    RepoFileWrite,
    #[value(name = "git-commit")]
    /// Runtime enum variant.
    GitCommit,
    #[value(name = "git-merge")]
    /// Runtime enum variant.
    GitMerge,
    #[value(name = "git-push")]
    /// Runtime enum variant.
    GitPush,
    #[value(name = "git-worktree-cleanup")]
    /// Runtime enum variant.
    GitWorktreeCleanup,
    #[value(name = "branch-finish")]
    /// Runtime enum variant.
    BranchFinish,
}

impl RepoSafetyWriteTargetArg {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SpecArtifactWrite => "spec-artifact-write",
            Self::PlanArtifactWrite => "plan-artifact-write",
            Self::ApprovalHeaderWrite => "approval-header-write",
            Self::ExecutionTaskSlice => "execution-task-slice",
            Self::ReleaseDocWrite => "release-doc-write",
            Self::RepoFileWrite => "repo-file-write",
            Self::GitCommit => "git-commit",
            Self::GitMerge => "git-merge",
            Self::GitPush => "git-push",
            Self::GitWorktreeCleanup => "git-worktree-cleanup",
            Self::BranchFinish => "branch-finish",
        }
    }
}
