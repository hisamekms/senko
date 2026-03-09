#!/usr/bin/env bash
# e2e test: Status transition validation (valid and invalid transitions)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Status Transitions ---"

# ===== Valid Transitions =====

echo "[1] Valid: draft → todo"
OUT="$(run_lf --output json add --title "Valid 1")"
ID="$(echo "$OUT" | jq -r '.id')"
OUT="$(run_lf --output json edit "$ID" --status todo)"
assert_json_field "$OUT" '.status' "todo" "draft → todo"

echo "[2] Valid: todo → in_progress"
OUT="$(run_lf --output json edit "$ID" --status in-progress)"
assert_json_field "$OUT" '.status' "in_progress" "todo → in_progress"

echo "[3] Valid: in_progress → completed"
OUT="$(run_lf --output json complete "$ID")"
assert_json_field "$OUT" '.status' "completed" "in_progress → completed"

echo "[4] Valid: draft → canceled"
OUT="$(run_lf --output json add --title "Valid 4")"
ID="$(echo "$OUT" | jq -r '.id')"
OUT="$(run_lf --output json cancel "$ID")"
assert_json_field "$OUT" '.status' "canceled" "draft → canceled"

echo "[5] Valid: todo → canceled"
OUT="$(run_lf --output json add --title "Valid 5")"
ID="$(echo "$OUT" | jq -r '.id')"
run_lf edit "$ID" --status todo >/dev/null
OUT="$(run_lf --output json cancel "$ID" --reason "不要")"
assert_json_field "$OUT" '.status' "canceled" "todo → canceled"

echo "[6] Valid: in_progress → canceled"
OUT="$(run_lf --output json add --title "Valid 6")"
ID="$(echo "$OUT" | jq -r '.id')"
run_lf edit "$ID" --status todo >/dev/null
run_lf edit "$ID" --status in-progress >/dev/null
OUT="$(run_lf --output json cancel "$ID" --reason "中止")"
assert_json_field "$OUT" '.status' "canceled" "in_progress → canceled"

# ===== Invalid Transitions =====

# Helper: create a task in a given status
create_task_in_status() {
  local status="$1"
  local out id
  out="$(run_lf --output json add --title "Task $status")"
  id="$(echo "$out" | jq -r '.id')"
  case "$status" in
    draft) ;;
    todo)
      run_lf edit "$id" --status todo >/dev/null
      ;;
    in_progress)
      run_lf edit "$id" --status todo >/dev/null
      run_lf edit "$id" --status in-progress >/dev/null
      ;;
    completed)
      run_lf edit "$id" --status todo >/dev/null
      run_lf edit "$id" --status in-progress >/dev/null
      run_lf complete "$id" >/dev/null
      ;;
    canceled)
      run_lf cancel "$id" >/dev/null
      ;;
  esac
  echo "$id"
}

echo "[7] Invalid: completed → todo"
ID="$(create_task_in_status completed)"
assert_exit_code 1 run_lf edit "$ID" --status todo

echo "[8] Invalid: completed → in_progress"
ID="$(create_task_in_status completed)"
assert_exit_code 1 run_lf edit "$ID" --status in-progress

echo "[9] Invalid: completed → draft"
ID="$(create_task_in_status completed)"
assert_exit_code 1 run_lf edit "$ID" --status draft

echo "[10] Invalid: canceled → todo"
ID="$(create_task_in_status canceled)"
assert_exit_code 1 run_lf edit "$ID" --status todo

echo "[11] Invalid: canceled → in_progress"
ID="$(create_task_in_status canceled)"
assert_exit_code 1 run_lf edit "$ID" --status in-progress

echo "[12] Invalid: canceled → draft"
ID="$(create_task_in_status canceled)"
assert_exit_code 1 run_lf edit "$ID" --status draft

echo "[13] Invalid: draft → in_progress (skip todo)"
ID="$(create_task_in_status draft)"
assert_exit_code 1 run_lf edit "$ID" --status in-progress

echo "[14] Invalid: draft → completed (skip intermediate)"
ID="$(create_task_in_status draft)"
assert_exit_code 1 run_lf complete "$ID"

echo "[15] Invalid: todo → completed (skip in_progress)"
ID="$(create_task_in_status todo)"
assert_exit_code 1 run_lf complete "$ID"

echo "[16] Invalid: todo → draft (backwards)"
ID="$(create_task_in_status todo)"
assert_exit_code 1 run_lf edit "$ID" --status draft

echo "[17] Invalid: in_progress → todo (backwards)"
ID="$(create_task_in_status in_progress)"
assert_exit_code 1 run_lf edit "$ID" --status todo

echo "[18] Invalid: in_progress → draft (backwards)"
ID="$(create_task_in_status in_progress)"
assert_exit_code 1 run_lf edit "$ID" --status draft

test_summary
