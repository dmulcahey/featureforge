use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::execution::transitions::{AuthoritativeTransitionState, ClosureHistorySnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ClosureScope {
    Task,
    Branch,
    Milestone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ClosureKind {
    TaskClosure,
    BranchClosure,
    ReleaseReadiness,
    FinalReview,
    BrowserQa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ClosureFreshness {
    Current,
    Superseded,
    StaleUnreviewed,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClosureIdentity {
    pub(crate) record_id: String,
    pub(crate) kind: ClosureKind,
    pub(crate) scope: ClosureScope,
    pub(crate) task_number: Option<u32>,
    pub(crate) plan_path: String,
    pub(crate) plan_fingerprint: String,
    pub(crate) repo_head_sha: Option<String>,
    pub(crate) tracked_tree_fingerprint: Option<String>,
    pub(crate) authoritative_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ClosureDependencyBinding {
    pub(crate) depends_on_record_ids: Vec<String>,
    pub(crate) source_artifact_fingerprints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClosureEvaluation {
    pub(crate) identity: ClosureIdentity,
    pub(crate) freshness: ClosureFreshness,
    pub(crate) supersedes: Option<String>,
    pub(crate) stale_reason_codes: Vec<String>,
    pub(crate) dependency_binding: ClosureDependencyBinding,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClosureGraphSignals {
    pub(crate) current_task_closure_ids: Vec<String>,
    pub(crate) current_branch_closure_id: Option<String>,
    pub(crate) overlay_current_branch_closure_id: Option<String>,
    pub(crate) finish_review_gate_pass_branch_closure_id: Option<String>,
    pub(crate) current_final_review_branch_closure_id: Option<String>,
    pub(crate) current_qa_branch_closure_id: Option<String>,
    pub(crate) late_stage_stale_unreviewed: bool,
    pub(crate) missing_current_closure_stale_provenance: bool,
    pub(crate) stale_reason_codes: Vec<String>,
}

impl ClosureGraphSignals {
    pub(crate) fn from_authoritative_state(
        authoritative_state: Option<&AuthoritativeTransitionState>,
        overlay_current_branch_closure_id: Option<&str>,
        late_stage_stale_unreviewed: bool,
        missing_current_closure_stale_provenance: bool,
        stale_reason_codes: Vec<String>,
    ) -> Self {
        let current_task_closure_ids = authoritative_state
            .map(|state| {
                state
                    .current_task_closure_results()
                    .into_values()
                    .map(|record| record.closure_record_id.trim().to_owned())
                    .filter(|record_id| !record_id.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let current_branch_closure_id = authoritative_state
            .and_then(|state| state.bound_current_branch_closure_identity())
            .map(|identity| identity.branch_closure_id);
        let finish_review_gate_pass_branch_closure_id = authoritative_state
            .and_then(AuthoritativeTransitionState::finish_review_gate_pass_branch_closure_id);
        let current_final_review_branch_closure_id = authoritative_state
            .and_then(AuthoritativeTransitionState::current_final_review_record)
            .map(|record| record.branch_closure_id);
        let current_qa_branch_closure_id = authoritative_state
            .and_then(AuthoritativeTransitionState::current_browser_qa_record)
            .map(|record| record.branch_closure_id);
        Self {
            current_task_closure_ids,
            current_branch_closure_id,
            overlay_current_branch_closure_id: overlay_current_branch_closure_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            finish_review_gate_pass_branch_closure_id,
            current_final_review_branch_closure_id,
            current_qa_branch_closure_id,
            late_stage_stale_unreviewed,
            missing_current_closure_stale_provenance,
            stale_reason_codes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AuthoritativeClosureGraph {
    evaluations: BTreeMap<String, ClosureEvaluation>,
    superseded_by: BTreeMap<String, String>,
    stale_projection_only_record_ids: Vec<String>,
    bound_current_branch_record_id: Option<String>,
    bound_current_release_readiness_record_id: Option<String>,
    bound_current_final_review_record_id: Option<String>,
    bound_current_browser_qa_record_id: Option<String>,
    current_task_record_ids: BTreeMap<u32, String>,
    current_branch_record_id: Option<String>,
    current_release_readiness_record_id: Option<String>,
    current_final_review_record_id: Option<String>,
    current_browser_qa_record_id: Option<String>,
}

impl AuthoritativeClosureGraph {
    pub(crate) fn from_state(
        authoritative_state: Option<&AuthoritativeTransitionState>,
        signals: &ClosureGraphSignals,
    ) -> Self {
        let snapshot = authoritative_state
            .map(AuthoritativeTransitionState::closure_history_snapshot)
            .unwrap_or_default();
        Self::from_snapshot(&snapshot, signals)
    }

    pub(crate) fn from_snapshot(
        snapshot: &ClosureHistorySnapshot,
        signals: &ClosureGraphSignals,
    ) -> Self {
        let mut graph = Self::default();
        graph.ingest_task_closure_history(&snapshot.task_closure_record_history);
        graph.ingest_branch_closure_history(
            &snapshot.branch_closure_records,
            &snapshot.superseded_branch_closure_ids,
        );
        graph.ingest_release_readiness_history(&snapshot.release_readiness_record_history);
        graph.ingest_final_review_history(&snapshot.final_review_record_history);
        graph.ingest_browser_qa_history(&snapshot.browser_qa_record_history);
        graph.bound_current_branch_record_id = snapshot.current_branch_closure_id.clone();
        graph.bound_current_release_readiness_record_id =
            snapshot.current_release_readiness_record_id.clone();
        graph.bound_current_final_review_record_id =
            snapshot.current_final_review_record_id.clone();
        graph.bound_current_browser_qa_record_id = snapshot.current_qa_record_id.clone();
        graph.apply_superseded_task_ids(&snapshot.superseded_task_closure_ids);
        graph.apply_late_stage_stale_projection(signals);
        graph.refresh_current_indexes(signals);
        graph
    }

    pub(crate) fn current_task_closure(&self, task: u32) -> Option<&ClosureEvaluation> {
        self.current_task_record_ids
            .get(&task)
            .and_then(|record_id| self.evaluations.get(record_id))
    }

    pub(crate) fn current_branch_closure(&self) -> Option<&ClosureEvaluation> {
        self.current_branch_record_id
            .as_ref()
            .and_then(|record_id| self.evaluations.get(record_id))
    }

    pub(crate) fn current_release_readiness_record_id(&self) -> Option<&str> {
        self.current_release_readiness_record_id.as_deref()
    }

    pub(crate) fn current_final_review_record_id(&self) -> Option<&str> {
        self.current_final_review_record_id.as_deref()
    }

    pub(crate) fn current_browser_qa_record_id(&self) -> Option<&str> {
        self.current_browser_qa_record_id.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn current_release_readiness(&self) -> Option<&ClosureEvaluation> {
        self.current_release_readiness_record_id
            .as_ref()
            .and_then(|record_id| self.evaluations.get(record_id))
    }

    #[cfg(test)]
    pub(crate) fn current_final_review(&self) -> Option<&ClosureEvaluation> {
        self.current_final_review_record_id
            .as_ref()
            .and_then(|record_id| self.evaluations.get(record_id))
    }

    #[cfg(test)]
    pub(crate) fn current_browser_qa(&self) -> Option<&ClosureEvaluation> {
        self.current_browser_qa_record_id
            .as_ref()
            .and_then(|record_id| self.evaluations.get(record_id))
    }

    pub(crate) fn evaluation(&self, record_id: &str) -> Option<&ClosureEvaluation> {
        self.evaluations.get(record_id)
    }

    pub(crate) fn superseded_by(&self, record_id: &str) -> Option<&str> {
        self.superseded_by.get(record_id).map(String::as_str)
    }

    pub(crate) fn superseded_record_ids(&self) -> Vec<String> {
        self.evaluations
            .iter()
            .filter(|(_, evaluation)| evaluation.freshness == ClosureFreshness::Superseded)
            .map(|(record_id, _)| record_id.clone())
            .collect()
    }

    pub(crate) fn stale_unreviewed_record_ids(&self) -> Vec<String> {
        let mut stale_record_ids = self
            .evaluations
            .iter()
            .filter(|(_, evaluation)| evaluation.freshness == ClosureFreshness::StaleUnreviewed)
            .map(|(record_id, _)| record_id.clone())
            .collect::<Vec<_>>();
        for record_id in &self.stale_projection_only_record_ids {
            append_unique(&mut stale_record_ids, record_id.clone());
        }
        stale_record_ids
    }

    pub(crate) fn latest_stale_task_number(&self) -> Option<u32> {
        self.evaluations
            .values()
            .filter(|evaluation| {
                evaluation.identity.kind == ClosureKind::TaskClosure
                    && evaluation.freshness == ClosureFreshness::StaleUnreviewed
            })
            .filter_map(|evaluation| {
                evaluation
                    .identity
                    .task_number
                    .map(|task_number| (evaluation.identity.authoritative_sequence, task_number))
            })
            .max_by_key(|(sequence, task_number)| (*sequence, *task_number))
            .map(|(_, task_number)| task_number)
    }

    fn ingest_task_closure_history(&mut self, history: &BTreeMap<String, Value>) {
        for (history_key, payload) in history {
            let Some(task_number) = value_u32(payload, "task") else {
                continue;
            };
            let record_id = value_string(payload, "closure_record_id")
                .or_else(|| value_string(payload, "record_id"))
                .unwrap_or_else(|| history_key.clone());
            if record_id.trim().is_empty() {
                continue;
            }
            let dependency_binding = ClosureDependencyBinding {
                source_artifact_fingerprints: value_string_array(
                    payload,
                    "effective_reviewed_surface_paths",
                ),
                ..ClosureDependencyBinding::default()
            };
            let freshness = value_closure_freshness(payload).unwrap_or(ClosureFreshness::Current);
            let stale_reason_codes = stale_reason_codes_for_freshness(freshness);
            let evaluation = ClosureEvaluation {
                identity: ClosureIdentity {
                    record_id: record_id.clone(),
                    kind: ClosureKind::TaskClosure,
                    scope: ClosureScope::Task,
                    task_number: Some(task_number),
                    plan_path: value_string(payload, "source_plan_path").unwrap_or_default(),
                    plan_fingerprint: value_u32(payload, "source_plan_revision")
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    repo_head_sha: None,
                    tracked_tree_fingerprint: value_string(payload, "reviewed_state_id"),
                    authoritative_sequence: value_u64(payload, "record_sequence").unwrap_or(0),
                },
                freshness,
                supersedes: None,
                stale_reason_codes,
                dependency_binding,
            };
            self.upsert_evaluation(evaluation);
        }
    }

    fn ingest_branch_closure_history(
        &mut self,
        records: &BTreeMap<String, Value>,
        superseded_branch_closure_ids: &[String],
    ) {
        for (history_key, payload) in records {
            let record_id =
                value_string(payload, "branch_closure_id").unwrap_or_else(|| history_key.clone());
            if record_id.trim().is_empty() {
                continue;
            }
            let freshness = value_closure_freshness(payload).unwrap_or(ClosureFreshness::Current);
            let stale_reason_codes = stale_reason_codes_for_freshness(freshness);
            let source_artifact_fingerprints =
                value_string(payload, "effective_reviewed_branch_surface")
                    .filter(|surface| !surface.trim().is_empty())
                    .into_iter()
                    .collect();
            let dependency_binding = ClosureDependencyBinding {
                depends_on_record_ids: value_string_array(payload, "source_task_closure_ids"),
                source_artifact_fingerprints,
            };
            let mut evaluation = ClosureEvaluation {
                identity: ClosureIdentity {
                    record_id: record_id.clone(),
                    kind: ClosureKind::BranchClosure,
                    scope: ClosureScope::Branch,
                    task_number: None,
                    plan_path: value_string(payload, "source_plan_path").unwrap_or_default(),
                    plan_fingerprint: value_u32(payload, "source_plan_revision")
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    repo_head_sha: None,
                    tracked_tree_fingerprint: value_string(payload, "reviewed_state_id"),
                    authoritative_sequence: value_u64(payload, "record_sequence").unwrap_or(0),
                },
                freshness,
                supersedes: None,
                stale_reason_codes,
                dependency_binding,
            };
            let explicit_superseded = value_string_array(payload, "superseded_branch_closure_ids");
            if let Some(first_superseded) = explicit_superseded.first() {
                evaluation.supersedes = Some(first_superseded.clone());
            }
            for superseded_record_id in explicit_superseded {
                self.superseded_by
                    .insert(superseded_record_id, record_id.clone());
            }
            self.upsert_evaluation(evaluation);
        }
        for superseded_record_id in superseded_branch_closure_ids {
            if let Some(evaluation) = self.evaluations.get_mut(superseded_record_id)
                && evaluation.freshness == ClosureFreshness::Current
            {
                evaluation.freshness = ClosureFreshness::Superseded;
            }
        }
    }

    fn ingest_release_readiness_history(&mut self, history: &BTreeMap<String, Value>) {
        for (history_key, payload) in history {
            let record_id =
                value_string(payload, "record_id").unwrap_or_else(|| history_key.clone());
            if record_id.trim().is_empty() {
                continue;
            }
            let freshness = value_closure_freshness(payload).unwrap_or(ClosureFreshness::Current);
            let stale_reason_codes = stale_reason_codes_for_freshness(freshness);
            let dependency_binding = ClosureDependencyBinding {
                depends_on_record_ids: value_string(payload, "branch_closure_id")
                    .into_iter()
                    .collect(),
                source_artifact_fingerprints: value_string(payload, "release_docs_fingerprint")
                    .into_iter()
                    .collect(),
            };
            let evaluation = ClosureEvaluation {
                identity: ClosureIdentity {
                    record_id: record_id.clone(),
                    kind: ClosureKind::ReleaseReadiness,
                    scope: ClosureScope::Milestone,
                    task_number: None,
                    plan_path: value_string(payload, "source_plan_path").unwrap_or_default(),
                    plan_fingerprint: value_u32(payload, "source_plan_revision")
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    repo_head_sha: None,
                    tracked_tree_fingerprint: value_string(payload, "reviewed_state_id"),
                    authoritative_sequence: value_u64(payload, "record_sequence").unwrap_or(0),
                },
                freshness,
                supersedes: None,
                stale_reason_codes,
                dependency_binding,
            };
            self.upsert_evaluation(evaluation);
        }
    }

    fn ingest_final_review_history(&mut self, history: &BTreeMap<String, Value>) {
        for (history_key, payload) in history {
            let record_id =
                value_string(payload, "record_id").unwrap_or_else(|| history_key.clone());
            if record_id.trim().is_empty() {
                continue;
            }
            let freshness = value_closure_freshness(payload).unwrap_or(ClosureFreshness::Current);
            let stale_reason_codes = stale_reason_codes_for_freshness(freshness);
            let mut depends_on_record_ids = Vec::new();
            if let Some(branch_closure_id) = value_string(payload, "branch_closure_id") {
                depends_on_record_ids.push(branch_closure_id);
            }
            if let Some(release_readiness_record_id) =
                value_string(payload, "release_readiness_record_id")
            {
                append_unique(&mut depends_on_record_ids, release_readiness_record_id);
            }
            let dependency_binding = ClosureDependencyBinding {
                depends_on_record_ids,
                source_artifact_fingerprints: value_string(payload, "final_review_fingerprint")
                    .into_iter()
                    .collect(),
            };
            let evaluation = ClosureEvaluation {
                identity: ClosureIdentity {
                    record_id: record_id.clone(),
                    kind: ClosureKind::FinalReview,
                    scope: ClosureScope::Milestone,
                    task_number: None,
                    plan_path: value_string(payload, "source_plan_path").unwrap_or_default(),
                    plan_fingerprint: value_u32(payload, "source_plan_revision")
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    repo_head_sha: None,
                    tracked_tree_fingerprint: value_string(payload, "reviewed_state_id"),
                    authoritative_sequence: value_u64(payload, "record_sequence").unwrap_or(0),
                },
                freshness,
                supersedes: None,
                stale_reason_codes,
                dependency_binding,
            };
            self.upsert_evaluation(evaluation);
        }
    }

    fn ingest_browser_qa_history(&mut self, history: &BTreeMap<String, Value>) {
        for (history_key, payload) in history {
            let record_id =
                value_string(payload, "record_id").unwrap_or_else(|| history_key.clone());
            if record_id.trim().is_empty() {
                continue;
            }
            let freshness = value_closure_freshness(payload).unwrap_or(ClosureFreshness::Current);
            let stale_reason_codes = stale_reason_codes_for_freshness(freshness);
            let mut source_artifact_fingerprints = Vec::new();
            if let Some(browser_qa_fingerprint) = value_string(payload, "browser_qa_fingerprint") {
                source_artifact_fingerprints.push(browser_qa_fingerprint);
            }
            if let Some(source_test_plan_fingerprint) =
                value_string(payload, "source_test_plan_fingerprint")
            {
                source_artifact_fingerprints.push(source_test_plan_fingerprint);
            }
            let mut depends_on_record_ids = Vec::new();
            if let Some(branch_closure_id) = value_string(payload, "branch_closure_id") {
                depends_on_record_ids.push(branch_closure_id);
            }
            if let Some(final_review_record_id) = value_string(payload, "final_review_record_id") {
                append_unique(&mut depends_on_record_ids, final_review_record_id);
            }
            let dependency_binding = ClosureDependencyBinding {
                depends_on_record_ids,
                source_artifact_fingerprints,
            };
            let evaluation = ClosureEvaluation {
                identity: ClosureIdentity {
                    record_id: record_id.clone(),
                    kind: ClosureKind::BrowserQa,
                    scope: ClosureScope::Milestone,
                    task_number: None,
                    plan_path: value_string(payload, "source_plan_path").unwrap_or_default(),
                    plan_fingerprint: value_u32(payload, "source_plan_revision")
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    repo_head_sha: None,
                    tracked_tree_fingerprint: value_string(payload, "reviewed_state_id"),
                    authoritative_sequence: value_u64(payload, "record_sequence").unwrap_or(0),
                },
                freshness,
                supersedes: None,
                stale_reason_codes,
                dependency_binding,
            };
            self.upsert_evaluation(evaluation);
        }
    }

    fn apply_superseded_task_ids(&mut self, superseded_task_closure_ids: &[String]) {
        for superseded_record_id in superseded_task_closure_ids {
            if let Some(evaluation) = self.evaluations.get_mut(superseded_record_id)
                && evaluation.freshness == ClosureFreshness::Current
            {
                evaluation.freshness = ClosureFreshness::Superseded;
            }
        }
    }

    fn apply_late_stage_stale_projection(&mut self, signals: &ClosureGraphSignals) {
        if !(signals.late_stage_stale_unreviewed
            || signals.missing_current_closure_stale_provenance)
        {
            return;
        }

        let stale_targets = late_stage_candidate_closure_ids_from_signals(signals);
        for record_id in stale_targets {
            if let Some(evaluation) = self.evaluations.get_mut(&record_id) {
                if matches!(
                    evaluation.freshness,
                    ClosureFreshness::Current | ClosureFreshness::Superseded
                ) {
                    evaluation.freshness = ClosureFreshness::StaleUnreviewed;
                }
                for reason_code in &signals.stale_reason_codes {
                    append_unique(&mut evaluation.stale_reason_codes, reason_code.clone());
                }
            } else {
                append_unique(&mut self.stale_projection_only_record_ids, record_id);
            }
        }
    }

    fn refresh_current_indexes(&mut self, _signals: &ClosureGraphSignals) {
        self.current_task_record_ids = self
            .evaluations
            .values()
            .filter(|evaluation| {
                evaluation.identity.kind == ClosureKind::TaskClosure
                    && evaluation.freshness == ClosureFreshness::Current
            })
            .fold(
                BTreeMap::<u32, (u64, String)>::new(),
                |mut current, evaluation| {
                    let Some(task_number) = evaluation.identity.task_number else {
                        return current;
                    };
                    let sequence = evaluation.identity.authoritative_sequence;
                    let entry = current
                        .entry(task_number)
                        .or_insert_with(|| (sequence, evaluation.identity.record_id.clone()));
                    if sequence >= entry.0 {
                        *entry = (sequence, evaluation.identity.record_id.clone());
                    }
                    current
                },
            )
            .into_iter()
            .map(|(task, (_, record_id))| (task, record_id))
            .collect();

        self.current_branch_record_id = self.bound_current_record_id(
            self.bound_current_branch_record_id.as_deref(),
            ClosureKind::BranchClosure,
        );
        let current_branch_record_id = self.current_branch_record_id.as_deref();
        self.current_release_readiness_record_id = self.bound_current_milestone_record_id(
            self.bound_current_release_readiness_record_id.as_deref(),
            ClosureKind::ReleaseReadiness,
            current_branch_record_id,
            None,
        );
        self.current_final_review_record_id = self
            .current_release_readiness_record_id
            .as_deref()
            .and_then(|release_record_id| {
                self.bound_current_milestone_record_id(
                    self.bound_current_final_review_record_id.as_deref(),
                    ClosureKind::FinalReview,
                    current_branch_record_id,
                    Some(release_record_id),
                )
            });
        self.current_browser_qa_record_id = self
            .current_final_review_record_id
            .as_deref()
            .and_then(|final_review_record_id| {
                self.bound_current_milestone_record_id(
                    self.bound_current_browser_qa_record_id.as_deref(),
                    ClosureKind::BrowserQa,
                    current_branch_record_id,
                    Some(final_review_record_id),
                )
            });

        let task_supersession_edges = self
            .evaluations
            .values()
            .filter(|evaluation| {
                evaluation.identity.kind == ClosureKind::TaskClosure
                    && matches!(
                        evaluation.freshness,
                        ClosureFreshness::Superseded | ClosureFreshness::Historical
                    )
            })
            .filter_map(|evaluation| {
                let task_number = evaluation.identity.task_number?;
                let current_record_id = self.current_task_record_ids.get(&task_number)?;
                (current_record_id != &evaluation.identity.record_id).then_some((
                    evaluation.identity.record_id.clone(),
                    current_record_id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        for (superseded_record_id, current_record_id) in task_supersession_edges {
            self.superseded_by
                .entry(superseded_record_id)
                .or_insert(current_record_id);
        }

        for evaluation in self.evaluations.values() {
            if evaluation.freshness != ClosureFreshness::Current {
                continue;
            }
            if let Some(superseded_record_id) = evaluation.supersedes.clone() {
                self.superseded_by
                    .insert(superseded_record_id, evaluation.identity.record_id.clone());
            }
        }
    }

    fn bound_current_record_id(
        &self,
        bound_record_id: Option<&str>,
        kind: ClosureKind,
    ) -> Option<String> {
        let bound_record_id = bound_record_id?.trim();
        if bound_record_id.is_empty() {
            return None;
        }
        let evaluation = self.evaluations.get(bound_record_id)?;
        if evaluation.identity.kind != kind || evaluation.freshness != ClosureFreshness::Current {
            return None;
        }
        Some(bound_record_id.to_owned())
    }

    fn bound_current_milestone_record_id(
        &self,
        bound_record_id: Option<&str>,
        kind: ClosureKind,
        branch_closure_id: Option<&str>,
        required_dependency_id: Option<&str>,
    ) -> Option<String> {
        let bound_record_id = bound_record_id?.trim();
        if bound_record_id.is_empty() {
            return None;
        }
        let branch_closure_id = branch_closure_id?.trim();
        if branch_closure_id.is_empty() {
            return None;
        }
        let evaluation = self.evaluations.get(bound_record_id)?;
        if evaluation.identity.kind != kind || evaluation.freshness != ClosureFreshness::Current {
            return None;
        }
        let primary_dependency = evaluation
            .dependency_binding
            .depends_on_record_ids
            .first()
            .map(String::as_str);
        if primary_dependency != Some(branch_closure_id) {
            return None;
        }
        if let Some(required_dependency_id) = required_dependency_id
            && !evaluation
                .dependency_binding
                .depends_on_record_ids
                .iter()
                .any(|record_id| record_id == required_dependency_id)
        {
            return None;
        }
        Some(bound_record_id.to_owned())
    }

    fn upsert_evaluation(&mut self, evaluation: ClosureEvaluation) {
        let record_id = evaluation.identity.record_id.clone();
        let keep_existing = self.evaluations.get(&record_id).is_some_and(|existing| {
            existing.identity.authoritative_sequence > evaluation.identity.authoritative_sequence
        });
        if !keep_existing {
            self.evaluations.insert(record_id, evaluation);
        }
    }
}

fn late_stage_candidate_closure_ids_from_signals(signals: &ClosureGraphSignals) -> Vec<String> {
    let mut closure_ids = BTreeSet::new();
    for closure_id in [
        signals.current_branch_closure_id.as_deref(),
        signals.overlay_current_branch_closure_id.as_deref(),
        signals.finish_review_gate_pass_branch_closure_id.as_deref(),
        signals.current_final_review_branch_closure_id.as_deref(),
        signals.current_qa_branch_closure_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let closure_id = closure_id.trim();
        if closure_id.is_empty() {
            continue;
        }
        closure_ids.insert(closure_id.to_owned());
    }
    if closure_ids.is_empty() {
        closure_ids.extend(signals.current_task_closure_ids.iter().cloned());
    }
    closure_ids.into_iter().collect()
}

fn stale_reason_codes_for_freshness(freshness: ClosureFreshness) -> Vec<String> {
    if freshness == ClosureFreshness::StaleUnreviewed {
        vec![String::from("record_status_stale_unreviewed")]
    } else {
        Vec::new()
    }
}

fn value_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn value_u64(payload: &Value, key: &str) -> Option<u64> {
    payload.get(key).and_then(Value::as_u64)
}

fn value_u32(payload: &Value, key: &str) -> Option<u32> {
    value_u64(payload, key)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn value_string_array(payload: &Value, key: &str) -> Vec<String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn value_closure_freshness(payload: &Value) -> Option<ClosureFreshness> {
    value_string(payload, "closure_status")
        .or_else(|| value_string(payload, "record_status"))
        .and_then(|status| closure_freshness_from_status(&status))
}

fn closure_freshness_from_status(status: &str) -> Option<ClosureFreshness> {
    match status {
        "current" => Some(ClosureFreshness::Current),
        "superseded" => Some(ClosureFreshness::Superseded),
        "stale_unreviewed" => Some(ClosureFreshness::StaleUnreviewed),
        "historical" => Some(ClosureFreshness::Historical),
        _ => None,
    }
}

pub(crate) fn reason_code_indicates_stale_unreviewed(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "review_artifact_worktree_dirty"
            | "post_review_repo_write_detected"
            | "release_docs_state_stale"
            | "release_docs_state_not_fresh"
            | "final_review_state_stale"
            | "final_review_state_not_fresh"
            | "browser_qa_state_stale"
            | "browser_qa_state_not_fresh"
            | "plain_unit_review_receipt_fingerprint_mismatch"
            | "files_proven_drifted"
    ) || reason_code.ends_with("_stale")
        || reason_code.ends_with("_not_fresh")
}

fn append_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task_status(record_id: &str, task: u32, status: &str, sequence: u64) -> Value {
        json!({
            "record_id": record_id,
            "closure_record_id": record_id,
            "task": task,
            "record_status": status,
            "closure_status": status,
            "record_sequence": sequence,
            "reviewed_state_id": format!("git_tree:{record_id}"),
            "contract_identity": format!("task-contract-{task}"),
            "effective_reviewed_surface_paths": ["src/lib.rs"],
        })
    }

    #[test]
    fn selects_current_over_superseded_task_closure() {
        let snapshot = ClosureHistorySnapshot {
            task_closure_record_history: BTreeMap::from([
                (
                    String::from("task-1-old"),
                    task_status("task-1-old", 1, "superseded", 1),
                ),
                (
                    String::from("task-1-current"),
                    task_status("task-1-current", 1, "current", 2),
                ),
            ]),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        let current = graph
            .current_task_closure(1)
            .expect("task closure should exist");
        assert_eq!(current.identity.record_id, "task-1-current");
        assert_eq!(current.freshness, ClosureFreshness::Current);
        assert_eq!(
            graph.superseded_record_ids(),
            vec![String::from("task-1-old")]
        );
    }

    #[test]
    fn marks_late_stage_targets_stale_unreviewed_from_signals() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "closure_status": "current",
                    "record_sequence": 5,
                    "reviewed_state_id": "git_tree:branch",
                    "source_task_closure_ids": ["task-1-current"],
                    "superseded_branch_closure_ids": [],
                }),
            )]),
            ..ClosureHistorySnapshot::default()
        };
        let signals = ClosureGraphSignals {
            current_branch_closure_id: Some(String::from("branch-current")),
            late_stage_stale_unreviewed: true,
            stale_reason_codes: vec![String::from("files_proven_drifted")],
            ..ClosureGraphSignals::default()
        };
        let graph = AuthoritativeClosureGraph::from_snapshot(&snapshot, &signals);
        let branch = graph
            .evaluation("branch-current")
            .expect("branch closure should exist");
        assert_eq!(branch.freshness, ClosureFreshness::StaleUnreviewed);
        assert!(
            branch
                .stale_reason_codes
                .iter()
                .any(|code| code == "files_proven_drifted")
        );
    }

    #[test]
    fn binds_final_review_and_qa_to_current_upstream_milestones() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 1,
                }),
            )]),
            release_readiness_record_history: BTreeMap::from([(
                String::from("release-current"),
                json!({
                    "record_id": "release-current",
                    "record_status": "current",
                    "record_sequence": 2,
                    "branch_closure_id": "branch-current",
                }),
            )]),
            final_review_record_history: BTreeMap::from([(
                String::from("final-current"),
                json!({
                    "record_id": "final-current",
                    "record_status": "current",
                    "record_sequence": 3,
                    "branch_closure_id": "branch-current",
                    "release_readiness_record_id": "release-current",
                }),
            )]),
            browser_qa_record_history: BTreeMap::from([(
                String::from("qa-current"),
                json!({
                    "record_id": "qa-current",
                    "record_status": "current",
                    "record_sequence": 4,
                    "branch_closure_id": "branch-current",
                    "final_review_record_id": "final-current",
                }),
            )]),
            current_branch_closure_id: Some(String::from("branch-current")),
            current_release_readiness_record_id: Some(String::from("release-current")),
            current_final_review_record_id: Some(String::from("final-current")),
            current_qa_record_id: Some(String::from("qa-current")),
            ..ClosureHistorySnapshot::default()
        };

        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        let release = graph
            .current_release_readiness()
            .expect("release readiness should exist");
        assert_eq!(release.identity.record_id, "release-current");
        let final_review = graph
            .current_final_review()
            .expect("final review should exist");
        assert!(
            final_review
                .dependency_binding
                .depends_on_record_ids
                .iter()
                .any(|record_id| record_id == "release-current")
        );
        let qa = graph.current_browser_qa().expect("qa should exist");
        assert!(
            qa.dependency_binding
                .depends_on_record_ids
                .iter()
                .any(|record_id| record_id == "final-current")
        );
    }

    #[test]
    fn status_signal_wins_when_overlay_branch_closure_disagrees() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([
                (
                    String::from("branch-old"),
                    json!({
                        "branch_closure_id": "branch-old",
                        "record_status": "current",
                        "record_sequence": 1,
                    }),
                ),
                (
                    String::from("branch-current"),
                    json!({
                        "branch_closure_id": "branch-current",
                        "record_status": "current",
                        "record_sequence": 2,
                    }),
                ),
            ]),
            current_branch_closure_id: Some(String::from("branch-current")),
            ..ClosureHistorySnapshot::default()
        };
        let signals = ClosureGraphSignals {
            current_branch_closure_id: Some(String::from("branch-current")),
            overlay_current_branch_closure_id: Some(String::from("branch-old")),
            ..ClosureGraphSignals::default()
        };
        let graph = AuthoritativeClosureGraph::from_snapshot(&snapshot, &signals);
        let current = graph
            .current_branch_closure()
            .expect("current branch closure should exist");
        assert_eq!(current.identity.record_id, "branch-current");
    }

    #[test]
    fn tracks_explicit_branch_supersession_lineage() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 2,
                    "superseded_branch_closure_ids": ["branch-old"],
                }),
            )]),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert_eq!(graph.superseded_by("branch-old"), Some("branch-current"));
    }

    #[test]
    fn graph_uses_explicit_current_branch_binding_without_history_inference() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([
                (
                    String::from("branch-bound"),
                    json!({
                        "branch_closure_id": "branch-bound",
                        "record_status": "current",
                        "record_sequence": 1,
                    }),
                ),
                (
                    String::from("branch-decoy"),
                    json!({
                        "branch_closure_id": "branch-decoy",
                        "record_status": "current",
                        "record_sequence": 2,
                    }),
                ),
            ]),
            current_branch_closure_id: Some(String::from("branch-bound")),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert_eq!(
            graph
                .current_branch_closure()
                .expect("bound branch should remain current")
                .identity
                .record_id,
            "branch-bound",
        );
    }

    #[test]
    fn missing_bound_branch_id_does_not_recover_from_history() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 1,
                }),
            )]),
            current_branch_closure_id: None,
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert!(
            graph.current_branch_closure().is_none(),
            "current branch closure must be explicitly bound via current_branch_closure_id"
        );
    }

    #[test]
    fn missing_bound_milestone_ids_do_not_recover_from_history() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 1,
                }),
            )]),
            release_readiness_record_history: BTreeMap::from([(
                String::from("release-current"),
                json!({
                    "record_id": "release-current",
                    "record_status": "current",
                    "record_sequence": 2,
                    "branch_closure_id": "branch-current",
                }),
            )]),
            final_review_record_history: BTreeMap::from([(
                String::from("final-current"),
                json!({
                    "record_id": "final-current",
                    "record_status": "current",
                    "record_sequence": 3,
                    "branch_closure_id": "branch-current",
                    "release_readiness_record_id": "release-current",
                }),
            )]),
            browser_qa_record_history: BTreeMap::from([(
                String::from("qa-current"),
                json!({
                    "record_id": "qa-current",
                    "record_status": "current",
                    "record_sequence": 4,
                    "branch_closure_id": "branch-current",
                    "final_review_record_id": "final-current",
                }),
            )]),
            current_branch_closure_id: None,
            current_release_readiness_record_id: None,
            current_final_review_record_id: None,
            current_qa_record_id: None,
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert!(
            graph.current_release_readiness().is_none(),
            "release-readiness current identity must be explicitly persisted"
        );
        assert!(
            graph.current_final_review().is_none(),
            "final-review current identity must be explicitly persisted"
        );
        assert!(
            graph.current_browser_qa().is_none(),
            "browser-QA current identity must be explicitly persisted"
        );
    }

    #[test]
    fn does_not_synthesize_current_branch_from_overlay_signals() {
        let signals = ClosureGraphSignals {
            overlay_current_branch_closure_id: Some(String::from("branch-from-overlay")),
            finish_review_gate_pass_branch_closure_id: Some(String::from("branch-from-gate")),
            ..ClosureGraphSignals::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&ClosureHistorySnapshot::default(), &signals);
        assert!(
            graph.current_branch_closure().is_none(),
            "current branch closure must come only from authoritative branch history",
        );
    }

    #[test]
    fn latest_stale_task_number_uses_authoritative_stale_task_target() {
        let snapshot = ClosureHistorySnapshot {
            task_closure_record_history: BTreeMap::from([
                (
                    String::from("task-4"),
                    task_status("task-4", 4, "stale_unreviewed", 10),
                ),
                (
                    String::from("task-6"),
                    task_status("task-6", 6, "stale_unreviewed", 20),
                ),
                (
                    String::from("task-7"),
                    task_status("task-7", 7, "current", 30),
                ),
            ]),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert_eq!(graph.latest_stale_task_number(), Some(6));
    }

    #[test]
    fn milestone_current_selection_is_scoped_to_current_branch_closure() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([
                (
                    String::from("branch-a"),
                    json!({
                        "branch_closure_id": "branch-a",
                        "record_status": "current",
                        "record_sequence": 1,
                    }),
                ),
                (
                    String::from("branch-b"),
                    json!({
                        "branch_closure_id": "branch-b",
                        "record_status": "historical",
                        "record_sequence": 2,
                    }),
                ),
            ]),
            release_readiness_record_history: BTreeMap::from([
                (
                    String::from("release-a"),
                    json!({
                        "record_id": "release-a",
                        "record_status": "current",
                        "record_sequence": 1,
                        "branch_closure_id": "branch-a",
                    }),
                ),
                (
                    String::from("release-b"),
                    json!({
                        "record_id": "release-b",
                        "record_status": "current",
                        "record_sequence": 2,
                        "branch_closure_id": "branch-b",
                    }),
                ),
            ]),
            final_review_record_history: BTreeMap::from([
                (
                    String::from("final-a"),
                    json!({
                        "record_id": "final-a",
                        "record_status": "current",
                        "record_sequence": 1,
                        "branch_closure_id": "branch-a",
                        "release_readiness_record_id": "release-a",
                    }),
                ),
                (
                    String::from("final-b"),
                    json!({
                        "record_id": "final-b",
                        "record_status": "current",
                        "record_sequence": 2,
                        "branch_closure_id": "branch-b",
                        "release_readiness_record_id": "release-b",
                    }),
                ),
            ]),
            browser_qa_record_history: BTreeMap::from([
                (
                    String::from("qa-a"),
                    json!({
                        "record_id": "qa-a",
                        "record_status": "current",
                        "record_sequence": 1,
                        "branch_closure_id": "branch-a",
                        "final_review_record_id": "final-a",
                    }),
                ),
                (
                    String::from("qa-b"),
                    json!({
                        "record_id": "qa-b",
                        "record_status": "current",
                        "record_sequence": 2,
                        "branch_closure_id": "branch-b",
                        "final_review_record_id": "final-b",
                    }),
                ),
            ]),
            current_branch_closure_id: Some(String::from("branch-a")),
            current_release_readiness_record_id: Some(String::from("release-a")),
            current_final_review_record_id: Some(String::from("final-a")),
            current_qa_record_id: Some(String::from("qa-a")),
            ..ClosureHistorySnapshot::default()
        };
        let signals = ClosureGraphSignals {
            current_branch_closure_id: Some(String::from("branch-a")),
            ..ClosureGraphSignals::default()
        };
        let graph = AuthoritativeClosureGraph::from_snapshot(&snapshot, &signals);
        assert_eq!(
            graph
                .current_release_readiness()
                .expect("release should exist")
                .identity
                .record_id,
            "release-a"
        );
        assert_eq!(
            graph
                .current_final_review()
                .expect("final review should exist")
                .identity
                .record_id,
            "final-a"
        );
        assert_eq!(
            graph
                .current_browser_qa()
                .expect("qa should exist")
                .identity
                .record_id,
            "qa-a"
        );
    }

    #[test]
    fn final_review_is_not_current_without_current_release_readiness_dependency() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 1,
                }),
            )]),
            final_review_record_history: BTreeMap::from([(
                String::from("final-current"),
                json!({
                    "record_id": "final-current",
                    "record_status": "current",
                    "record_sequence": 2,
                    "branch_closure_id": "branch-current",
                }),
            )]),
            current_branch_closure_id: Some(String::from("branch-current")),
            current_final_review_record_id: Some(String::from("final-current")),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert!(
            graph.current_final_review().is_none(),
            "final review should not be current without current release-readiness dependency",
        );
    }

    #[test]
    fn bound_final_review_record_without_explicit_release_dependency_is_not_inferred_current() {
        let snapshot = ClosureHistorySnapshot {
            branch_closure_records: BTreeMap::from([(
                String::from("branch-current"),
                json!({
                    "branch_closure_id": "branch-current",
                    "record_status": "current",
                    "record_sequence": 1,
                }),
            )]),
            release_readiness_record_history: BTreeMap::from([(
                String::from("release-current"),
                json!({
                    "record_id": "release-current",
                    "record_status": "current",
                    "record_sequence": 2,
                    "branch_closure_id": "branch-current",
                }),
            )]),
            final_review_record_history: BTreeMap::from([(
                String::from("final-current"),
                json!({
                    "record_id": "final-current",
                    "record_status": "current",
                    "record_sequence": 3,
                    "branch_closure_id": "branch-current",
                }),
            )]),
            current_branch_closure_id: Some(String::from("branch-current")),
            current_release_readiness_record_id: Some(String::from("release-current")),
            current_final_review_record_id: Some(String::from("final-current")),
            ..ClosureHistorySnapshot::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&snapshot, &ClosureGraphSignals::default());
        assert!(
            graph.current_final_review().is_none(),
            "final review must not be inferred current when release dependency binding is absent",
        );
    }

    #[test]
    fn status_only_stale_targets_do_not_create_authoritative_nodes() {
        let signals = ClosureGraphSignals {
            current_branch_closure_id: Some(String::from("missing-branch")),
            late_stage_stale_unreviewed: true,
            stale_reason_codes: vec![String::from("files_proven_drifted")],
            ..ClosureGraphSignals::default()
        };
        let graph =
            AuthoritativeClosureGraph::from_snapshot(&ClosureHistorySnapshot::default(), &signals);
        assert!(
            graph.evaluation("missing-branch").is_none(),
            "status-only stale targets must remain diagnostic and must not create authoritative closure nodes",
        );
    }
}
