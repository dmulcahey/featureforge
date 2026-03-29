_MESSAGE_LC=$(tr '[:upper:]' '[:lower:]' < "$SP_TEST_MESSAGE_FILE")
_EXPLICIT_PROJECT_MEMORY_ROUTE=""
if printf '%s' "$_MESSAGE_LC" | grep -Eq 'featureforge:project-memory|project memory itself|(?:set up|repair) project memory|(?:log|record) (?:this |a )?bug fix|record (?:this |a )?decision|update (?:our )?key facts|record durable bugs|record durable decisions|record durable key facts|record key facts|record durable issue breadcrumbs|record issue breadcrumbs|(?:set up|repair) docs/project_notes/|docs/project_notes/(readme|bugs|decisions|key_facts|issues)\.md'; then
  _EXPLICIT_PROJECT_MEMORY_ROUTE="featureforge:project-memory"
fi
if [ -n "$_EXPLICIT_PROJECT_MEMORY_ROUTE" ]; then
  printf '%s\n' "$_EXPLICIT_PROJECT_MEMORY_ROUTE"
elif [ -n "${SP_TEST_WORKFLOW_NEXT_SKILL:-}" ]; then
  printf '%s\n' "$SP_TEST_WORKFLOW_NEXT_SKILL"
fi
