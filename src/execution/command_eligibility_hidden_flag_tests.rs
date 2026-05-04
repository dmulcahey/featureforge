use super::*;

#[test]
fn internal_flags_are_unrepresentable_as_typed_public_commands() {
    let prefix = "featureforge plan execution";
    let dispatch_id = concat!("--dispatch", "-id");
    let branch_closure_id = concat!("--branch", "-closure-id");
    let commands = [
        format!("{prefix} begin --plan docs/plan.md --task 1 --step 2 {dispatch_id} dispatch-1"),
        format!(
            "{prefix} complete --plan docs/plan.md --task 1 --step 2 {dispatch_id} dispatch-1 --claim done --manual-verify-summary summary.md"
        ),
        format!(
            "{prefix} close-current-task --plan docs/plan.md --task 1 {dispatch_id} dispatch-1 --review-result pass --review-summary-file review.md --verification-result pass"
        ),
        format!("{prefix} advance-late-stage --plan docs/plan.md {dispatch_id} dispatch-1"),
        format!(
            "{prefix} advance-late-stage --plan docs/plan.md {branch_closure_id} branch-closure-1"
        ),
    ];

    for command in &commands {
        assert!(
            PublicCommand::parse_display_command(command).is_none(),
            "internal flag must not parse as typed public command: {command}"
        );
        assert!(!command_is_legal_public_command(command));
        assert!(public_mutation_request_from_command(command).is_none());
    }
}
