#!/usr/bin/env bash
# E2E tests for environment variable overrides (12-Factor App)
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

setup_test_env
trap cleanup_test_env EXIT

echo "--- Test: Environment Variable Overrides ---"

echo "[1] SENKO_MERGE_VIA overrides default"
JSON_OUT="$(SENKO_MERGE_VIA=pr run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "pr" "env overrides merge_via"

echo "[2] SENKO_AUTO_MERGE overrides default"
JSON_OUT="$(SENKO_AUTO_MERGE=false run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "env overrides auto_merge"

echo "[3] SENKO_CLI_REMOTE_URL overrides default"
JSON_OUT="$(SENKO_CLI_REMOTE_URL=http://remote:9999 run_lf config)"
assert_json_field "$JSON_OUT" '.cli.remote.url' "http://remote:9999" "env overrides cli.remote.url"

echo "[4] Env vars override config.toml values"
mkdir -p "$TEST_PROJECT_ROOT/.senko"
cat > "$TEST_PROJECT_ROOT/.senko/config.toml" <<'EOF'
[workflow]
merge_via = "direct"
auto_merge = true
EOF
JSON_OUT="$(SENKO_MERGE_VIA=pr SENKO_AUTO_MERGE=false run_lf config)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "pr" "env overrides toml merge_via"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "env overrides toml auto_merge"

# Note: SENKO_HOOKS_ENABLED and SENKO_HOOK_ON_TASK_* env vars were removed in
# the hooks-config-refresh change. Hooks are now configured exclusively via
# config.toml under [cli.*] / [server.remote.*] / [server.relay.*] / [workflow.*].
rm -f "$TEST_PROJECT_ROOT/.senko/config.toml"

echo "[9] SENKO_PROJECT_ROOT overrides --project-root"
ALT_PROJECT="$(mktemp -d)"
ALT_DB="$ALT_PROJECT/.senko/data.db"
# Initialize a DB via senko in the alt project
"$SENKO" --project-root "$ALT_PROJECT" --db-path "$ALT_DB" task add --title "Alt project task" >/dev/null
JSON_OUT="$(SENKO_PROJECT_ROOT=$ALT_PROJECT SENKO_DB_PATH=$ALT_DB "$SENKO" task list)"
TASK_TITLE=$(echo "$JSON_OUT" | jq -r '.items[0].title')
assert_eq "Alt project task" "$TASK_TITLE" "SENKO_PROJECT_ROOT selects alt project"
rm -rf "$ALT_PROJECT"

echo "[10] SENKO_PORT sets serve port"
PORT=$(allocate_port 0)
SENKO_PORT=$PORT SENKO_AUTH_API_KEY_MASTER_KEY=test-key "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null || true; cleanup_test_env' EXIT
wait_for "serve with SENKO_PORT" 10 "curl -sf http://127.0.0.1:$PORT/api/v1/health >/dev/null"
echo "  PASS: serve started on port $PORT via SENKO_PORT"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo "[11] SENKO_HOST sets bind address"
PORT2=$(allocate_port 1)
SENKO_HOST=127.0.0.1 SENKO_AUTH_API_KEY_MASTER_KEY=test-key "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT2" &
SERVER_PID2=$!
trap 'kill $SERVER_PID2 2>/dev/null || true; cleanup_test_env' EXIT
wait_for "serve with SENKO_HOST" 10 "curl -sf http://127.0.0.1:$PORT2/api/v1/health >/dev/null"
echo "  PASS: serve started with SENKO_HOST=127.0.0.1"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID2 2>/dev/null || true
wait $SERVER_PID2 2>/dev/null || true

echo "[12] Config works with no config.toml (env-only)"
NO_TOML_PROJECT="$(mktemp -d)"
JSON_OUT="$(SENKO_MERGE_VIA=pr SENKO_AUTO_MERGE=false "$SENKO" --project-root "$NO_TOML_PROJECT" config)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "pr" "no toml + env merge_via"
assert_json_field "$JSON_OUT" '.workflow.auto_merge' "false" "no toml + env auto_merge"
rm -rf "$NO_TOML_PROJECT"

echo "[13] CLI flags take priority over env vars"
PORT3=$(allocate_port 2)
PORT_CLI=$(allocate_port 3)
SENKO_PORT=$PORT3 SENKO_AUTH_API_KEY_MASTER_KEY=test-key "$SENKO" --project-root "$TEST_PROJECT_ROOT" --db-path "$TEST_PROJECT_ROOT/.senko/data.db" serve --port "$PORT_CLI" &
SERVER_PID3=$!
trap 'kill $SERVER_PID3 2>/dev/null || true; cleanup_test_env' EXIT
wait_for "serve with CLI port override" 10 "curl -sf http://127.0.0.1:$PORT_CLI/api/v1/health >/dev/null"
echo "  PASS: CLI --port overrides SENKO_PORT"
PASS_COUNT=$((PASS_COUNT + 1))
kill $SERVER_PID3 2>/dev/null || true
wait $SERVER_PID3 2>/dev/null || true

echo "[14] backward compat: SENKO_COMPLETION_MODE still works (deprecated)"
rm -f "$TEST_PROJECT_ROOT/.senko/config.toml"
JSON_OUT="$(SENKO_COMPLETION_MODE=pr_then_complete run_lf config 2>/dev/null)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "pr" "deprecated SENKO_COMPLETION_MODE still works"

echo "[15] backward compat: old values in SENKO_MERGE_VIA still work"
JSON_OUT="$(SENKO_MERGE_VIA=merge_then_complete run_lf config 2>/dev/null)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "direct" "old value merge_then_complete via SENKO_MERGE_VIA"

echo "[16] SENKO_MERGE_VIA takes priority over SENKO_COMPLETION_MODE"
JSON_OUT="$(SENKO_MERGE_VIA=direct SENKO_COMPLETION_MODE=pr_then_complete run_lf config 2>/dev/null)"
assert_json_field "$JSON_OUT" '.workflow.merge_via' "direct" "SENKO_MERGE_VIA wins over SENKO_COMPLETION_MODE"

test_summary
