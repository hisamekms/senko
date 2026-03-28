#!/usr/bin/env bash
# E2E tests for environment variable overrides (12-Factor App)
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Environment Variable Overrides ---"

echo "[1] LOCALFLOW_COMPLETION_MODE overrides default"
JSON_OUT="$(LOCALFLOW_COMPLETION_MODE=pr_then_complete run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.completion_mode' "pr_then_complete" "env overrides completion_mode"

echo "[2] LOCALFLOW_AUTO_MERGE overrides default"
JSON_OUT="$(LOCALFLOW_AUTO_MERGE=false run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "env overrides auto_merge"

echo "[3] LOCALFLOW_HOOK_MODE overrides default"
JSON_OUT="$(LOCALFLOW_HOOK_MODE=client run_lf config)"
assert_json_field "$JSON_OUT" '.backend.hook_mode' "client" "env overrides hook_mode"

echo "[4] LOCALFLOW_API_URL overrides default"
JSON_OUT="$(LOCALFLOW_API_URL=http://remote:9999 run_lf config)"
assert_json_field "$JSON_OUT" '.backend.api_url' "http://remote:9999" "env overrides api_url"

echo "[5] Env vars override config.toml values"
mkdir -p "$TEST_PROJECT_ROOT/.localflow"
cat > "$TEST_PROJECT_ROOT/.localflow/config.toml" <<'EOF'
[workflow]
completion_mode = "merge_then_complete"
auto_merge = true

[backend]
hook_mode = "server"
EOF
JSON_OUT="$(LOCALFLOW_COMPLETION_MODE=pr_then_complete LOCALFLOW_AUTO_MERGE=false LOCALFLOW_HOOK_MODE=both run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.completion_mode' "pr_then_complete" "env overrides toml completion_mode"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "env overrides toml auto_merge"
assert_json_field "$JSON_OUT" '.backend.hook_mode' "both" "env overrides toml hook_mode"

echo "[6] LOCALFLOW_HOOK_ON_TASK_ADDED inserts env hook"
JSON_OUT="$(LOCALFLOW_HOOK_ON_TASK_ADDED="echo env-hook" run_lf config)"
HOOK_COUNT=$(echo "$JSON_OUT" | jq '.hooks.on_task_added | keys | length')
assert_eq "1" "$HOOK_COUNT" "env hook inserted (no toml hooks)"
ENV_CMD=$(echo "$JSON_OUT" | jq -r '.hooks.on_task_added._env.command')
assert_eq "echo env-hook" "$ENV_CMD" "env hook command"

echo "[7] LOCALFLOW_HOOK_ON_TASK_ADDED alongside config.toml hooks"
cat > "$TEST_PROJECT_ROOT/.localflow/config.toml" <<'EOF'
[hooks.on_task_added.toml-hook]
command = "echo toml-hook"
EOF
JSON_OUT="$(LOCALFLOW_HOOK_ON_TASK_ADDED="echo env-hook" run_lf config)"
HOOK_COUNT=$(echo "$JSON_OUT" | jq '.hooks.on_task_added | keys | length')
assert_eq "2" "$HOOK_COUNT" "env hook alongside toml hook"
TOML_CMD=$(echo "$JSON_OUT" | jq -r '.hooks.on_task_added["toml-hook"].command')
ENV_CMD=$(echo "$JSON_OUT" | jq -r '.hooks.on_task_added._env.command')
assert_eq "echo toml-hook" "$TOML_CMD" "toml hook command"
assert_eq "echo env-hook" "$ENV_CMD" "env hook command"

echo "[8] All 5 hook env vars work"
rm -f "$TEST_PROJECT_ROOT/.localflow/config.toml"
JSON_OUT="$(LOCALFLOW_HOOK_ON_TASK_ADDED="cmd1" \
  LOCALFLOW_HOOK_ON_TASK_READY="cmd2" \
  LOCALFLOW_HOOK_ON_TASK_STARTED="cmd3" \
  LOCALFLOW_HOOK_ON_TASK_COMPLETED="cmd4" \
  LOCALFLOW_HOOK_ON_TASK_CANCELED="cmd5" \
  run_lf config)"
assert_json_field "$JSON_OUT" '.hooks.on_task_added._env.command' "cmd1" "on_task_added env"
assert_json_field "$JSON_OUT" '.hooks.on_task_ready._env.command' "cmd2" "on_task_ready env"
assert_json_field "$JSON_OUT" '.hooks.on_task_started._env.command' "cmd3" "on_task_started env"
assert_json_field "$JSON_OUT" '.hooks.on_task_completed._env.command' "cmd4" "on_task_completed env"
assert_json_field "$JSON_OUT" '.hooks.on_task_canceled._env.command' "cmd5" "on_task_canceled env"

echo "[9] LOCALFLOW_PROJECT_ROOT overrides --project-root"
ALT_PROJECT="$(mktemp -d)"
# Initialize a DB via localflow in the alt project
"$LOCALFLOW" --project-root "$ALT_PROJECT" add --title "Alt project task" >/dev/null
JSON_OUT="$(LOCALFLOW_PROJECT_ROOT=$ALT_PROJECT "$LOCALFLOW" list)"
TASK_TITLE=$(echo "$JSON_OUT" | jq -r '.[0].title')
assert_eq "Alt project task" "$TASK_TITLE" "LOCALFLOW_PROJECT_ROOT selects alt project"
rm -rf "$ALT_PROJECT"

echo "[10] LOCALFLOW_PORT sets serve port"
PORT=$((20000 + RANDOM % 40000))
LOCALFLOW_PORT=$PORT "$LOCALFLOW" --project-root "$TEST_PROJECT_ROOT" serve &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; cleanup_test_env' EXIT
wait_for "serve with LOCALFLOW_PORT" 10 "curl -sf http://127.0.0.1:$PORT/api/v1/stats >/dev/null"
echo "  PASS: serve started on port $PORT via LOCALFLOW_PORT"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo "[11] LOCALFLOW_HOST sets bind address"
PORT2=$((20000 + RANDOM % 40000))
LOCALFLOW_HOST=127.0.0.1 "$LOCALFLOW" --project-root "$TEST_PROJECT_ROOT" serve --port "$PORT2" &
SERVER_PID2=$!
trap 'kill $SERVER_PID2 2>/dev/null; cleanup_test_env' EXIT
wait_for "serve with LOCALFLOW_HOST" 10 "curl -sf http://127.0.0.1:$PORT2/api/v1/stats >/dev/null"
echo "  PASS: serve started with LOCALFLOW_HOST=127.0.0.1"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID2 2>/dev/null || true
wait $SERVER_PID2 2>/dev/null || true

echo "[12] Config works with no config.toml (env-only)"
NO_TOML_PROJECT="$(mktemp -d)"
JSON_OUT="$(LOCALFLOW_COMPLETION_MODE=pr_then_complete LOCALFLOW_AUTO_MERGE=false "$LOCALFLOW" --project-root "$NO_TOML_PROJECT" config)"
assert_json_field "$JSON_OUT" '.workflow.completion_mode' "pr_then_complete" "no toml + env completion_mode"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "no toml + env auto_merge"
rm -rf "$NO_TOML_PROJECT"

echo "[13] CLI flags take priority over env vars"
PORT3=$((20000 + RANDOM % 40000))
PORT_CLI=$((20000 + RANDOM % 40000))
# Ensure different ports
while [[ "$PORT_CLI" == "$PORT3" ]]; do
  PORT_CLI=$((20000 + RANDOM % 40000))
done
LOCALFLOW_PORT=$PORT3 "$LOCALFLOW" --project-root "$TEST_PROJECT_ROOT" serve --port "$PORT_CLI" &
SERVER_PID3=$!
trap 'kill $SERVER_PID3 2>/dev/null; cleanup_test_env' EXIT
wait_for "serve with CLI port override" 10 "curl -sf http://127.0.0.1:$PORT_CLI/api/v1/stats >/dev/null"
echo "  PASS: CLI --port overrides LOCALFLOW_PORT"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID3 2>/dev/null || true
wait $SERVER_PID3 2>/dev/null || true

test_summary
