use super::*;

fn stale_task_target(record_id: &str) -> AuthoritativeStaleTarget {
    AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Task,
        task: Some(2),
        step: None,
        record_id: Some(record_id.to_owned()),
        source: AuthoritativeStaleTargetSource::ClosureGraph,
        reason_code: String::from(
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
        ),
        task_closure_bridge_allowed: false,
    }
}

fn projection_only_milestone_target(record_id: &str) -> AuthoritativeStaleTarget {
    AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Milestone,
        task: None,
        step: None,
        record_id: Some(record_id.to_owned()),
        source: AuthoritativeStaleTargetSource::ProjectionOnly,
        reason_code: String::from("projection_only_stale_target"),
        task_closure_bridge_allowed: false,
    }
}

#[test]
fn current_task_closures_are_removed_not_used_as_fallback_stale_targets() {
    let mut targets = vec![
        stale_task_target("closure-current"),
        stale_task_target("closure-stale"),
    ];
    let current_closure_ids = BTreeSet::from(["closure-current"]);

    remove_current_task_closure_stale_targets_for_ids(&mut targets, &current_closure_ids);

    assert_eq!(targets, vec![stale_task_target("closure-stale")]);
}

#[test]
fn no_stale_targets_are_fabricated_from_current_closure_ids() {
    let mut targets = Vec::new();
    let current_closure_ids = BTreeSet::from(["closure-current"]);

    remove_current_task_closure_stale_targets_for_ids(&mut targets, &current_closure_ids);

    assert!(targets.is_empty());
}

#[test]
fn projection_only_targets_are_diagnostic_not_repair_authority() {
    let projection_only = projection_only_milestone_target("projection-only-branch");

    assert!(
        !projection_only.can_drive_public_repair(),
        "projection-only stale ids are diagnostics and must not be public repair authority"
    );
    assert!(
        !stale_closure_record_target(&projection_only),
        "projection-only stale ids must not populate stale_unreviewed_closures"
    );
    assert!(
        !authoritative_stale_target_present([&projection_only], false, false),
        "projection-only stale ids must not satisfy has_authoritative_stale_target"
    );
}
