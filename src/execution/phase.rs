//! Shared public execution phase and phase-detail vocabulary.
//!
//! Public JSON values remain stringly for compatibility, but new routing and
//! read-model code should source those strings from here instead of introducing
//! another local spelling.

pub const PHASE_BLOCKED: &str = "blocked";
pub const PHASE_DOCUMENT_RELEASE_PENDING: &str = "document_release_pending";
pub const PHASE_EXECUTING: &str = "executing";
pub const PHASE_EXECUTION_PREFLIGHT: &str = "execution_preflight";
pub const PHASE_FINAL_REVIEW_PENDING: &str = "final_review_pending";
pub const PHASE_HANDOFF_REQUIRED: &str = "handoff_required";
pub const PHASE_IMPLEMENTATION_HANDOFF: &str = "implementation_handoff";
pub const PHASE_PIVOT_REQUIRED: &str = "pivot_required";
pub const PHASE_QA_PENDING: &str = "qa_pending";
pub const PHASE_READY_FOR_BRANCH_COMPLETION: &str = "ready_for_branch_completion";
pub const PHASE_TASK_CLOSURE_PENDING: &str = "task_closure_pending";

pub const PUBLIC_STATUS_PHASE_VALUES: &[&str] = &[
    PHASE_BLOCKED,
    PHASE_DOCUMENT_RELEASE_PENDING,
    PHASE_EXECUTING,
    PHASE_EXECUTION_PREFLIGHT,
    PHASE_FINAL_REVIEW_PENDING,
    PHASE_HANDOFF_REQUIRED,
    PHASE_PIVOT_REQUIRED,
    PHASE_QA_PENDING,
    PHASE_READY_FOR_BRANCH_COMPLETION,
    PHASE_TASK_CLOSURE_PENDING,
];

pub const DETAIL_BLOCKED_RUNTIME_BUG: &str = "blocked_runtime_bug";
pub const DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS: &str =
    "branch_closure_recording_required_for_release_readiness";
pub const DETAIL_EXECUTION_IN_PROGRESS: &str = "execution_in_progress";
pub const DETAIL_EXECUTION_PREFLIGHT_REQUIRED: &str = "execution_preflight_required";
pub const DETAIL_EXECUTION_REENTRY_REQUIRED: &str = "execution_reentry_required";
pub const DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED: &str = "final_review_dispatch_required";
pub const DETAIL_FINAL_REVIEW_OUTCOME_PENDING: &str = "final_review_outcome_pending";
pub const DETAIL_FINAL_REVIEW_RECORDING_READY: &str = "final_review_recording_ready";
pub const DETAIL_FINISH_COMPLETION_GATE_READY: &str = "finish_completion_gate_ready";
pub const DETAIL_FINISH_REVIEW_GATE_READY: &str = "finish_review_gate_ready";
pub const DETAIL_HANDOFF_RECORDING_REQUIRED: &str = "handoff_recording_required";
pub const DETAIL_PLANNING_REENTRY_REQUIRED: &str = "planning_reentry_required";
pub const DETAIL_QA_RECORDING_REQUIRED: &str = "qa_recording_required";
pub const DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED: &str = "release_blocker_resolution_required";
pub const DETAIL_RELEASE_READINESS_RECORDING_READY: &str = "release_readiness_recording_ready";
pub const DETAIL_RUNTIME_RECONCILE_REQUIRED: &str = "runtime_reconcile_required";
pub const DETAIL_TASK_CLOSURE_RECORDING_READY: &str = "task_closure_recording_ready";
pub const DETAIL_TASK_REVIEW_RESULT_PENDING: &str = "task_review_result_pending";
pub const DETAIL_TEST_PLAN_REFRESH_REQUIRED: &str = "test_plan_refresh_required";

pub const PLAN_EXECUTION_STATUS_PHASE_DETAIL_VALUES: &[&str] = &[
    DETAIL_BLOCKED_RUNTIME_BUG,
    DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
    DETAIL_EXECUTION_IN_PROGRESS,
    DETAIL_EXECUTION_PREFLIGHT_REQUIRED,
    DETAIL_EXECUTION_REENTRY_REQUIRED,
    DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
    DETAIL_FINAL_REVIEW_OUTCOME_PENDING,
    DETAIL_FINAL_REVIEW_RECORDING_READY,
    DETAIL_FINISH_COMPLETION_GATE_READY,
    DETAIL_FINISH_REVIEW_GATE_READY,
    DETAIL_HANDOFF_RECORDING_REQUIRED,
    DETAIL_PLANNING_REENTRY_REQUIRED,
    DETAIL_QA_RECORDING_REQUIRED,
    DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
    DETAIL_RELEASE_READINESS_RECORDING_READY,
    DETAIL_RUNTIME_RECONCILE_REQUIRED,
    DETAIL_TASK_CLOSURE_RECORDING_READY,
    DETAIL_TASK_REVIEW_RESULT_PENDING,
    DETAIL_TEST_PLAN_REFRESH_REQUIRED,
];

pub const RECORDING_CONTEXT_PHASE_DETAILS: &[&str] = &[
    DETAIL_TASK_CLOSURE_RECORDING_READY,
    DETAIL_RELEASE_READINESS_RECORDING_READY,
    DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
    DETAIL_FINAL_REVIEW_RECORDING_READY,
];

pub const RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS: &[&str] = &[
    DETAIL_TASK_REVIEW_RESULT_PENDING,
    DETAIL_EXECUTION_IN_PROGRESS,
    DETAIL_RUNTIME_RECONCILE_REQUIRED,
    DETAIL_FINISH_REVIEW_GATE_READY,
    DETAIL_FINISH_COMPLETION_GATE_READY,
    DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
    DETAIL_FINAL_REVIEW_OUTCOME_PENDING,
    DETAIL_TEST_PLAN_REFRESH_REQUIRED,
];

pub const WORKFLOW_STATUS_IMPLEMENTATION_READY: &str = "implementation_ready";
