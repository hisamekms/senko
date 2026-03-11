#!/usr/bin/env bash
# e2e test: add subcommand variants

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Add Variants ---"

# 1. Minimal (title only) — verify defaults
echo "[1] Minimal task (title only)"
ADD_MIN="$(run_lf --output json add --title "Minimal Task")"

assert_json_field "$ADD_MIN" '.title' "Minimal Task" "title is set"
assert_json_field "$ADD_MIN" '.status' "draft" "default status is draft"
assert_json_field "$ADD_MIN" '.priority' "P2" "default priority is P2"
assert_json_field "$ADD_MIN" '.tags' "[]" "default tags is empty array"
assert_json_field "$ADD_MIN" '.dependencies' "[]" "default dependencies is empty array"
assert_json_field "$ADD_MIN" '.background' "null" "default background is null"
assert_json_field "$ADD_MIN" '.description' "null" "default description is null"
assert_json_field "$ADD_MIN" '.plan' "null" "default plan is null"
assert_json_field "$ADD_MIN" '.definition_of_done' "[]" "default definition_of_done is empty array"
assert_json_field "$ADD_MIN" '.in_scope' "[]" "default in_scope is empty array"
assert_json_field "$ADD_MIN" '.out_of_scope' "[]" "default out_of_scope is empty array"

# 2. All fields specified
echo "[2] Full task (all fields)"
# Create dependency task first
DEP_OUTPUT="$(run_lf --output json add --title "Dependency Task")"
DEP_ID="$(echo "$DEP_OUTPUT" | jq -r '.id')"

ADD_FULL="$(run_lf --output json add \
  --title "Full Task" \
  --background "bg" \
  --description "det" \
  --priority p1 \
  --tag t1 --tag t2 \
  --definition-of-done "dod1" --definition-of-done "dod2" \
  --in-scope "is1" \
  --out-of-scope "os1" \
  --depends-on "$DEP_ID")"

assert_json_field "$ADD_FULL" '.title' "Full Task" "full: title"
assert_json_field "$ADD_FULL" '.priority' "P1" "full: priority P1"
assert_json_field "$ADD_FULL" '.background' "bg" "full: background"
assert_json_field "$ADD_FULL" '.description' "det" "full: description"

TAGS="$(echo "$ADD_FULL" | jq -c '.tags')"
assert_eq '["t1","t2"]' "$TAGS" "full: tags"

DOD="$(echo "$ADD_FULL" | jq -c '[.definition_of_done[].content]')"
assert_eq '["dod1","dod2"]' "$DOD" "full: definition_of_done"

IN_SCOPE="$(echo "$ADD_FULL" | jq -c '.in_scope')"
assert_eq '["is1"]' "$IN_SCOPE" "full: in_scope"

OUT_SCOPE="$(echo "$ADD_FULL" | jq -c '.out_of_scope')"
assert_eq '["os1"]' "$OUT_SCOPE" "full: out_of_scope"

DEPS="$(echo "$ADD_FULL" | jq -c '.dependencies')"
assert_eq "[$DEP_ID]" "$DEPS" "full: dependencies"

# 3. --from-json (stdin)
echo "[3] Add from JSON (stdin)"
ADD_JSON="$(echo '{"title":"From JSON","background":"json-bg","priority":"P0","tags":["a","b"]}' | run_lf --output json add --from-json)"

assert_json_field "$ADD_JSON" '.title' "From JSON" "from-json: title"
assert_json_field "$ADD_JSON" '.background' "json-bg" "from-json: background"
assert_json_field "$ADD_JSON" '.priority' "P0" "from-json: priority"

JSON_TAGS="$(echo "$ADD_JSON" | jq -c '.tags')"
assert_eq '["a","b"]' "$JSON_TAGS" "from-json: tags"

# 4. --from-json-file (file path)
echo "[4] Add from JSON file"
JSON_FILE="$TEST_DIR/task_input.json"
cat > "$JSON_FILE" <<'JSONEOF'
{
  "title": "From JSON File",
  "description": "file-description",
  "priority": "P3",
  "definition_of_done": ["done1"]
}
JSONEOF

ADD_FILE="$(run_lf --output json add --from-json-file "$JSON_FILE")"

assert_json_field "$ADD_FILE" '.title' "From JSON File" "from-json-file: title"
assert_json_field "$ADD_FILE" '.description' "file-description" "from-json-file: description"
assert_json_field "$ADD_FILE" '.priority' "P3" "from-json-file: priority"

FILE_DOD="$(echo "$ADD_FILE" | jq -c '[.definition_of_done[].content]')"
assert_eq '["done1"]' "$FILE_DOD" "from-json-file: definition_of_done"

# 5. --from-json with all fields (in_scope, out_of_scope, branch, dependencies)
echo "[5] Add from JSON with all fields"
# Create a dependency for the JSON task
JSON_DEP_OUT="$(run_lf --output json add --title "JSON Dep")"
JSON_DEP_ID="$(echo "$JSON_DEP_OUT" | jq -r '.id')"

ADD_JSON_FULL="$(cat <<JSONEOF | run_lf --output json add --from-json
{
  "title": "JSON Full Fields",
  "background": "json-bg-full",
  "description": "json-description-full",
  "priority": "P1",
  "tags": ["x", "y"],
  "definition_of_done": ["check1", "check2"],
  "in_scope": ["scope-in-1", "scope-in-2"],
  "out_of_scope": ["scope-out-1"],
  "branch": "feature/json-test",
  "dependencies": [$JSON_DEP_ID]
}
JSONEOF
)"

assert_json_field "$ADD_JSON_FULL" '.title' "JSON Full Fields" "from-json-full: title"
assert_json_field "$ADD_JSON_FULL" '.background' "json-bg-full" "from-json-full: background"
assert_json_field "$ADD_JSON_FULL" '.description' "json-description-full" "from-json-full: description"
assert_json_field "$ADD_JSON_FULL" '.priority' "P1" "from-json-full: priority"
assert_json_field "$ADD_JSON_FULL" '.branch' "feature/json-test" "from-json-full: branch"

FULL_TAGS="$(echo "$ADD_JSON_FULL" | jq -c '.tags')"
assert_eq '["x","y"]' "$FULL_TAGS" "from-json-full: tags"

FULL_DOD="$(echo "$ADD_JSON_FULL" | jq -c '[.definition_of_done[].content]')"
assert_eq '["check1","check2"]' "$FULL_DOD" "from-json-full: definition_of_done"

FULL_IN="$(echo "$ADD_JSON_FULL" | jq -c '.in_scope')"
assert_eq '["scope-in-1","scope-in-2"]' "$FULL_IN" "from-json-full: in_scope"

FULL_OUT="$(echo "$ADD_JSON_FULL" | jq -c '.out_of_scope')"
assert_eq '["scope-out-1"]' "$FULL_OUT" "from-json-full: out_of_scope"

FULL_DEPS="$(echo "$ADD_JSON_FULL" | jq -c '.dependencies')"
assert_eq "[$JSON_DEP_ID]" "$FULL_DEPS" "from-json-full: dependencies"

# 6. Error cases
echo "[6] Error cases"
# No title
assert_exit_code 1 run_lf add
# Non-existent dependency
assert_exit_code 1 run_lf add --title "Bad Dep" --depends-on 99999

test_summary
