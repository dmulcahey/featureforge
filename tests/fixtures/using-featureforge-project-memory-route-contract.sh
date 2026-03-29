_MESSAGE_LC=$(tr '[:upper:]' '[:lower:]' < "$SP_TEST_MESSAGE_FILE")
_EXPLICIT_PROJECT_MEMORY_ROUTE=""
if printf '%s' "$_MESSAGE_LC" | grep -Eq "do not use featureforge:project-memory|don't use featureforge:project-memory|do not work on project memory itself|don't work on project memory itself"; then
  _EXPLICIT_PROJECT_MEMORY_ROUTE=""
elif printf '%s' "$_MESSAGE_LC" | grep -Eq '(?:use|invoke) featureforge:project-memory|work on project memory itself|(?:set up|repair) project memory|(?:log|record) (?:this |a )?bug fix in project memory|record (?:this |a )?decision in project memory|update (?:our )?key facts in project memory|record durable bugs in project memory|record durable decisions in project memory|record durable key facts in project memory|record key facts in project memory|record durable issue breadcrumbs in project memory|record issue breadcrumbs in project memory|record (?:durable )?bugs in docs/project_notes/bugs\.md|record (?:durable )?decisions? in docs/project_notes/decisions\.md|record (?:durable )?key facts in docs/project_notes/key_facts\.md|record (?:durable )?issue breadcrumbs in docs/project_notes/issues\.md|(?:set up|repair|update|edit|fix) docs/project_notes/(readme|bugs|decisions|key_facts|issues)\.md'; then
  _EXPLICIT_PROJECT_MEMORY_ROUTE="featureforge:project-memory"
fi
if [ -n "$_EXPLICIT_PROJECT_MEMORY_ROUTE" ]; then
  printf '%s\n' "$_EXPLICIT_PROJECT_MEMORY_ROUTE"
elif [ -n "${SP_TEST_WORKFLOW_NEXT_SKILL:-}" ]; then
  printf '%s\n' "$SP_TEST_WORKFLOW_NEXT_SKILL"
fi
