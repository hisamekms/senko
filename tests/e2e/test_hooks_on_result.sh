#!/usr/bin/env bash
# e2e test: task_select + on_result filter.
#
# on_result = "selected" fires only when task next picks a task.
# on_result = "none"     fires only when task next returns no eligible task.
# on_result = "any" (or unset) fires in both cases.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: task_select on_result filter ---"

MARKER_DIR="$TEST_DIR/hook-markers"
mkdir -p "$MARKER_DIR"

mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<EOF
[cli.task_select.hooks.on_none]
command = "touch $MARKER_DIR/none_fired"
mode = "sync"
on_result = "none"

[cli.task_select.hooks.on_selected]
command = "touch $MARKER_DIR/selected_fired"
mode = "sync"
on_result = "selected"

[cli.task_select.hooks.on_any]
command = "touch $MARKER_DIR/any_fired"
mode = "sync"
on_result = "any"
EOF

clear_markers() {
  rm -f "$MARKER_DIR"/*
}

# Initialize the project database
run_lf --output json task list >/dev/null 2>&1

# ---------------------------------------------------------------
# Case A: no eligible task → task next fails, on_result=none fires.
# ---------------------------------------------------------------
clear_markers

echo "[A1] task next with no ready task"
NEXT_EXIT=0
run_lf task next >/dev/null 2>&1 || NEXT_EXIT=$?
if [[ "$NEXT_EXIT" -ne 0 ]]; then
  echo "  PASS: task next exited non-zero (no eligible task)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task next unexpectedly succeeded"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[A2] on_result=none hook fired"
if [[ -f "$MARKER_DIR/none_fired" ]]; then
  echo "  PASS: none_fired marker exists"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: none_fired marker missing"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[A3] on_result=selected hook did NOT fire"
if [[ ! -f "$MARKER_DIR/selected_fired" ]]; then
  echo "  PASS: selected_fired marker absent"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: selected_fired marker unexpectedly exists"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[A4] on_result=any hook fired"
if [[ -f "$MARKER_DIR/any_fired" ]]; then
  echo "  PASS: any_fired marker exists (Case A)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: any_fired marker missing (Case A)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# ---------------------------------------------------------------
# Case B: a ready task exists → task next picks it; on_result=selected fires.
# ---------------------------------------------------------------
TASK_ID="$(run_lf --output json task add --title "Eligible task" | jq -r '.id')"
run_lf task publish "$TASK_ID" >/dev/null 2>&1

clear_markers

echo "[B1] task next with one ready task"
NEXT_B_EXIT=0
run_lf task next >/dev/null 2>&1 || NEXT_B_EXIT=$?
if [[ "$NEXT_B_EXIT" -eq 0 ]]; then
  echo "  PASS: task next succeeded"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: task next failed ($NEXT_B_EXIT)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[B2] on_result=selected hook fired"
if [[ -f "$MARKER_DIR/selected_fired" ]]; then
  echo "  PASS: selected_fired marker exists"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: selected_fired marker missing"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[B3] on_result=none hook did NOT fire"
if [[ ! -f "$MARKER_DIR/none_fired" ]]; then
  echo "  PASS: none_fired marker absent"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: none_fired marker unexpectedly exists"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

echo "[B4] on_result=any hook fired"
if [[ -f "$MARKER_DIR/any_fired" ]]; then
  echo "  PASS: any_fired marker exists (Case B)"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  echo "  FAIL: any_fired marker missing (Case B)"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

test_summary
