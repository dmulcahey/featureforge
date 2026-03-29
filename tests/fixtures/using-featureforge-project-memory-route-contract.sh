_MESSAGE_LC=$(tr '[:upper:]' '[:lower:]' < "$SP_TEST_MESSAGE_FILE")
_EXPLICIT_PROJECT_MEMORY_ROUTE=""
if printf '%s' "$_MESSAGE_LC" | grep -Eq "do not use featureforge:project-memory|don't use featureforge:project-memory|do not work on project memory itself|don't work on project memory itself"; then
  _EXPLICIT_PROJECT_MEMORY_ROUTE=""
elif printf '%s' "$_MESSAGE_LC" | grep -Eq '(?:use|invoke) featureforge:project-memory|project memory itself|(?:set up|repair) project memory|(?:log|record) (?:this |a )?bug fix|record (?:this |a )?decision|update (?:our )?key facts|record durable bugs|record durable decisions|record durable key facts|record key facts|record durable issue breadcrumbs|record issue breadcrumbs|(?:set up|repair|update|edit|fix|record) docs/project_notes/(readme|bugs|decisions|key_facts|issues)\.md'; then
  _EXPLICIT_PROJECT_MEMORY_ROUTE="featureforge:project-memory"
fi
if [ -n "$_EXPLICIT_PROJECT_MEMORY_ROUTE" ]; then
  printf '%s\n' "$_EXPLICIT_PROJECT_MEMORY_ROUTE"
elif [ -n "${SP_TEST_WORKFLOW_NEXT_SKILL:-}" ]; then
  printf '%s\n' "$SP_TEST_WORKFLOW_NEXT_SKILL"
fi
