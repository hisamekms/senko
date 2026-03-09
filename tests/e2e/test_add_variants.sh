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
assert_json_field "$ADD_MIN" '.details' "null" "default details is null"
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
  --details "det" \
  --priority p1 \
  --tag t1 --tag t2 \
  --definition-of-done "dod1" --definition-of-done "dod2" \
  --in-scope "is1" \
  --out-of-scope "os1" \
  --depends-on "$DEP_ID")"

assert_json_field "$ADD_FULL" '.title' "Full Task" "full: title"
assert_json_field "$ADD_FULL" '.priority' "P1" "full: priority P1"
assert_json_field "$ADD_FULL" '.background' "bg" "full: background"
assert_json_field "$ADD_FULL" '.details' "det" "full: details"

TAGS="$(echo "$ADD_FULL" | jq -c '.tags')"
assert_eq '["t1","t2"]' "$TAGS" "full: tags"

DOD="$(echo "$ADD_FULL" | jq -c '.definition_of_done')"
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
  "details": "file-details",
  "priority": "P3",
  "definition_of_done": ["done1"]
}
JSONEOF

ADD_FILE="$(run_lf --output json add --from-json-file "$JSON_FILE")"

assert_json_field "$ADD_FILE" '.title' "From JSON File" "from-json-file: title"
assert_json_field "$ADD_FILE" '.details' "file-details" "from-json-file: details"
assert_json_field "$ADD_FILE" '.priority' "P3" "from-json-file: priority"

FILE_DOD="$(echo "$ADD_FILE" | jq -c '.definition_of_done')"
assert_eq '["done1"]' "$FILE_DOD" "from-json-file: definition_of_done"

# 5. Error cases
echo "[5] Error cases"
# No title
assert_exit_code 1 run_lf add
# Non-existent dependency
assert_exit_code 1 run_lf add --title "Bad Dep" --depends-on 99999

test_summary
