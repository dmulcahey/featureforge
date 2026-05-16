//! Shared parser for persisted task-scope keys and task-prefixed runtime record IDs.
//!
//! Runtime state stores task-scoped maps under exact `task-<u32>` keys. Keep
//! parsing here so repair, query, transition-state, and route code reject the
//! same malformed keys instead of each accepting a slightly different prefix.
//! Some event record IDs also start with `task-<u32>` before a descriptive
//! suffix; expose that as a separate parser so exact keys stay exact.

const TASK_SCOPE_KEY_PREFIX: &str = "task-";

pub(crate) fn task_scope_key_task_number(scope_key: &str) -> Option<u32> {
    let raw = scope_key.strip_prefix(TASK_SCOPE_KEY_PREFIX)?;
    if raw.is_empty() || !raw.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u32>().ok()
}

pub(crate) fn task_scope_key_for_task(task: u32) -> String {
    format!("{TASK_SCOPE_KEY_PREFIX}{task}")
}

pub(crate) fn task_prefixed_record_id_task_number(record_id: &str) -> Option<u32> {
    let raw = record_id.strip_prefix(TASK_SCOPE_KEY_PREFIX)?;
    let digits = raw
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_task_scope_keys() {
        assert_eq!(task_scope_key_task_number("task-1"), Some(1));
        assert_eq!(task_scope_key_task_number("task-0"), Some(0));
        assert_eq!(task_scope_key_task_number("task-001"), Some(1));
        assert_eq!(
            task_scope_key_task_number("task-4294967295"),
            Some(u32::MAX)
        );
        assert_eq!(task_scope_key_for_task(7), "task-7");
    }

    #[test]
    fn rejects_non_exact_task_scope_keys() {
        for scope_key in [
            "",
            "task",
            "task-",
            "Task-1",
            "task-+1",
            "task--1",
            "task-1-extra",
            "task-1:closure",
            "task-4294967296",
            "task- 1",
            "branch-1",
            "1",
        ] {
            assert_eq!(
                task_scope_key_task_number(scope_key),
                None,
                "{scope_key:?} must not parse as an exact task-scope key",
            );
        }
    }

    #[test]
    fn parses_task_prefixed_runtime_record_ids_without_weakening_scope_keys() {
        assert_eq!(
            task_prefixed_record_id_task_number("task-1-current-closure"),
            Some(1)
        );
        assert_eq!(
            task_prefixed_record_id_task_number("task-2-stale-old-session"),
            Some(2)
        );
        assert_eq!(task_prefixed_record_id_task_number("task-3"), Some(3));
        assert_eq!(task_prefixed_record_id_task_number("branch-3"), None);
        assert_eq!(task_prefixed_record_id_task_number("task-current"), None);
    }
}
