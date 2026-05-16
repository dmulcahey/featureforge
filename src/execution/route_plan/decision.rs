use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandInputRequirement, PublicCommandInvocation,
    public_command_recommendation_surfaces,
};
use crate::execution::public_command_types::{
    RecommendedPublicCommandArgv, RecommendedPublicCommandTemplate,
};
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
};
use crate::execution::state::{PlanExecutionStatus, PublicRepairTarget};

use super::route_semantics::{
    ExecutionBlockingProjectionInputs, external_wait_state_for_phase_detail,
    project_execution_blocking,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NextPublicAction {
    #[serde(default = "next_public_action_is_display_only")]
    pub display_only: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_template: Option<String>,
}

impl NextPublicAction {
    pub(super) fn display_summary(command: String) -> Self {
        Self {
            display_only: true,
            command: command.clone(),
            args_template: Some(command),
        }
    }
}

fn next_public_action_is_display_only() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Blocker {
    pub category: String,
    pub scope_type: String,
    pub scope_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<NextPublicAction>,
    pub details: String,
}

pub(crate) type RouteDecision = PublicRouteDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PublicRouteDecision {
    pub(crate) state_kind: String,
    pub(crate) phase: String,
    pub(crate) phase_detail: String,
    pub(crate) review_state_status: String,
    pub(crate) next_action: String,
    pub(crate) blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) external_wait_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_command: Option<String>,
    #[serde(skip)]
    pub(crate) recommended_public_command: Option<PublicCommand>,
    #[serde(skip)]
    pub(crate) invocation: Option<PublicCommandInvocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_public_command_template: RecommendedPublicCommandTemplate,
    pub(crate) required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) required_follow_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_public_action: Option<NextPublicAction>,
    pub(crate) blockers: Vec<Blocker>,
    #[serde(skip)]
    pub(crate) public_repair_targets: Vec<PublicRepairTarget>,
    #[serde(skip)]
    pub(crate) execution_reentry_target_source: Option<String>,
    #[serde(skip)]
    pub(crate) execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    #[serde(skip)]
    pub(crate) recording_context: Option<ExecutionRoutingRecordingContext>,
}

impl PublicRouteDecision {
    pub(crate) fn command_surfaces(
        command: Option<&PublicCommand>,
    ) -> (
        Option<String>,
        Option<PublicCommandInvocation>,
        RecommendedPublicCommandTemplate,
        Vec<PublicCommandInputRequirement>,
    ) {
        let (recommended_command, argv, template, required_inputs) =
            public_command_recommendation_surfaces(command);
        (
            recommended_command,
            argv.map(|argv| PublicCommandInvocation { argv }),
            template,
            required_inputs,
        )
    }

    pub(crate) fn public_command_argv(&self) -> RecommendedPublicCommandArgv {
        self.invocation
            .as_ref()
            .map(|invocation| invocation.argv.clone())
    }

    pub(crate) fn public_command_template(&self) -> RecommendedPublicCommandTemplate {
        self.recommended_public_command_template.clone()
    }

    pub(crate) fn recommended_command_display(&self) -> Option<String> {
        self.recommended_command.clone()
    }

    pub(crate) fn normalize_diagnostic_next_action(&mut self) {
        super::status_projection::normalize_diagnostic_route_decision(self);
    }

    pub(crate) fn apply_public_route_projection(
        &mut self,
        status: Option<&PlanExecutionStatus>,
        external_review_result_ready: bool,
    ) {
        let blocking_projection = project_execution_blocking(ExecutionBlockingProjectionInputs {
            phase_detail: &self.phase_detail,
            review_state_status: &self.review_state_status,
            status,
            fallback_scope: self
                .blocking_scope
                .as_deref()
                .or_else(|| status.and_then(|status| status.blocking_scope.as_deref())),
            fallback_task: self
                .blocking_task
                .or_else(|| status.and_then(|status| status.blocking_task)),
            execution_command_task: self
                .execution_command_context
                .as_ref()
                .and_then(|context| context.task_number),
            recording_task: self
                .recording_context
                .as_ref()
                .and_then(|context| context.task_number),
            blocker_task: super::blockers::blocking_task_from_blockers(&self.blockers),
        });
        self.blocking_scope = blocking_projection.scope;
        self.blocking_task = blocking_projection.task;
        self.external_wait_state = external_wait_state_for_phase_detail(
            &self.phase_detail,
            &self.blocking_reason_codes,
            external_review_result_ready,
        )
        .or_else(|| status.and_then(|status| status.external_wait_state.clone()));
    }
}
