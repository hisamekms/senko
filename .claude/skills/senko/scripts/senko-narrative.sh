#!/usr/bin/env bash
# Narrative + Review Packet helper for senko's pre-publication review flow.
#
# Stores per-registration state under $SENKO_STATE_DIR (default:
# $XDG_STATE_HOME/senko or ~/.local/state/senko) keyed by an 8-char alnum ID.
# The orchestrator passes the ID forward and resolves narrative.md / packet.md
# paths via the `path` and `packet-path` subcommands; reviewers Read those
# files instead of having JSON embedded in their prompt.
#
# Usage:
#   echo '{"intent":"..."}'  | senko-narrative.sh init                                  # → ID
#   echo '{"q":"...","a":"..."}' | senko-narrative.sh append-decision <id>
#   echo '{"note":"..."}'        | senko-narrative.sh append-decision <id>
#   echo '{"text":"..."}'        | senko-narrative.sh append-constraint <id>
#   senko-narrative.sh path <id>                                                        # → narrative.md path
#   senko-narrative.sh packet-path <id>                                                 # → packet.md path
#   senko-narrative.sh build-packet <id> --mode {split|single} [--contract <cid>] --tasks <id1> [<id2> ...]
#                                                                                        # → packet.md path
#
# Environment:
#   SENKO_BIN              Path to senko binary (default: 'senko' from PATH).
#   SENKO_STATE_DIR        Override state dir root.
#   SENKO_GC_CUTOFF_DAYS   Lazy-GC cutoff in days (default: 7).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/gc.sh
source "$SCRIPT_DIR/lib/gc.sh"

ID_REGEX='^[a-zA-Z0-9]{8}$'

senko_bin() {
  echo "${SENKO_BIN:-senko}"
}

validate_id() {
  local id="$1"
  if ! [[ "$id" =~ $ID_REGEX ]]; then
    echo "error: invalid id '$id' (must match $ID_REGEX)" >&2
    exit 2
  fi
}

ensure_state_dir() {
  local sd
  sd="$(senko_state_dir)"
  mkdir -p "$sd"
  echo "$sd"
}

require_id_dir() {
  local id="$1"
  validate_id "$id"
  local sd
  sd="$(senko_state_dir)"
  local dir="$sd/$id"
  if [ ! -d "$dir" ]; then
    echo "error: id '$id' not found at $dir" >&2
    exit 3
  fi
  echo "$dir"
}

now_iso() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

mtime_of() {
  if stat -c %Y "$1" >/dev/null 2>&1; then
    stat -c %Y "$1"
  else
    stat -f %m "$1"
  fi
}

lazy_gc() {
  local sd
  sd="$(senko_state_dir)"
  [ -d "$sd" ] || return 0
  local marker="$sd/.last-gc"
  local should_gc=0
  if [ ! -f "$marker" ]; then
    should_gc=1
  else
    local marker_mtime now age
    marker_mtime="$(mtime_of "$marker")"
    now="$(date +%s)"
    age=$((now - marker_mtime))
    if [ "$age" -ge 3600 ]; then
      should_gc=1
    fi
  fi
  if [ "$should_gc" = 1 ]; then
    gc_run
    touch "$marker"
  fi
}

# insert_into_section <file> <heading-text> <line>
# Idempotent: if <line> is already present anywhere in the file, no-op.
# Inserts <line> at the end of the named section (just before the next "# "
# heading or EOF) followed by a blank line.
insert_into_section() {
  local file="$1" heading="$2" line="$3"

  if grep -Fxq -- "$line" "$file" 2>/dev/null; then
    return 0
  fi

  local tmp
  tmp="$(mktemp)"
  awk -v h="# $heading" -v l="$line" '
    BEGIN { in_section=0; inserted=0 }
    /^# / {
      if (in_section && !inserted) {
        print l
        print ""
        inserted=1
      }
      in_section = ($0 == h)
      print
      next
    }
    { print }
    END {
      if (in_section && !inserted) {
        print l
      }
    }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
}

cmd_init() {
  local sd
  sd="$(ensure_state_dir)"

  local input
  input="$(cat)"
  local intent
  intent="$(echo "$input" | jq -r '.intent // ""')"
  if [ -z "$intent" ]; then
    echo 'error: stdin must be JSON like {"intent":"..."}' >&2
    exit 2
  fi

  local id_path id
  id_path="$(mktemp -d "$sd/XXXXXXXX")"
  id="$(basename "$id_path")"

  if ! [[ "$id" =~ $ID_REGEX ]]; then
    rm -rf "$id_path"
    echo "error: mktemp generated invalid id '$id'" >&2
    exit 4
  fi

  local now
  now="$(now_iso)"
  jq -n --arg created "$now" \
    '{mode: null, contract_id: null, task_ids: null, created_at: $created, updated_at: $created}' \
    > "$id_path/metadata.json"

  cat > "$id_path/narrative.md" <<EOF
# Original user intent

$intent

# Decisions

# Known constraints

EOF

  lazy_gc

  echo "$id"
}

cmd_append_decision() {
  local id="${1:-}"
  if [ -z "$id" ]; then
    echo "error: usage: append-decision <id>" >&2
    exit 2
  fi
  local dir
  dir="$(require_id_dir "$id")"
  local input
  input="$(cat)"

  local line
  if echo "$input" | jq -e 'has("q") and has("a")' > /dev/null 2>&1; then
    local q a
    q="$(echo "$input" | jq -r '.q')"
    a="$(echo "$input" | jq -r '.a')"
    line="- $q: $a"
  elif echo "$input" | jq -e 'has("note")' > /dev/null 2>&1; then
    local note
    note="$(echo "$input" | jq -r '.note')"
    line="- (note) $note"
  else
    echo 'error: stdin must be {"q":"...","a":"..."} or {"note":"..."}' >&2
    exit 2
  fi

  insert_into_section "$dir/narrative.md" "Decisions" "$line"
  touch "$dir/metadata.json"
}

cmd_append_constraint() {
  local id="${1:-}"
  if [ -z "$id" ]; then
    echo "error: usage: append-constraint <id>" >&2
    exit 2
  fi
  local dir
  dir="$(require_id_dir "$id")"
  local input
  input="$(cat)"

  local text
  text="$(echo "$input" | jq -r '.text // ""')"
  if [ -z "$text" ]; then
    echo 'error: stdin must be {"text":"..."}' >&2
    exit 2
  fi

  insert_into_section "$dir/narrative.md" "Known constraints" "- $text"
  touch "$dir/metadata.json"
}

cmd_path() {
  local id="${1:-}"
  if [ -z "$id" ]; then
    echo "error: usage: path <id>" >&2
    exit 2
  fi
  local dir
  dir="$(require_id_dir "$id")"
  echo "$dir/narrative.md"
}

cmd_packet_path() {
  local id="${1:-}"
  if [ -z "$id" ]; then
    echo "error: usage: packet-path <id>" >&2
    exit 2
  fi
  local dir
  dir="$(require_id_dir "$id")"
  echo "$dir/packet.md"
}

cmd_build_packet() {
  local id="${1:-}"
  if [ -z "$id" ]; then
    echo "error: usage: build-packet <id> --mode {split|single} [--contract <cid>] --tasks <id1> ..." >&2
    exit 2
  fi
  shift
  local dir
  dir="$(require_id_dir "$id")"

  local mode="" contract=""
  local tasks=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --mode)
        if [ $# -lt 2 ]; then echo "error: --mode requires a value" >&2; exit 2; fi
        mode="$2"; shift 2 ;;
      --contract)
        if [ $# -lt 2 ]; then echo "error: --contract requires a value" >&2; exit 2; fi
        contract="$2"; shift 2 ;;
      --tasks)
        shift
        while [ $# -gt 0 ] && [[ ! "$1" =~ ^-- ]]; do
          tasks+=("$1"); shift
        done
        ;;
      *)
        echo "error: unknown arg '$1'" >&2; exit 2 ;;
    esac
  done

  if [[ ! "$mode" =~ ^(split|single)$ ]]; then
    echo "error: --mode must be 'split' or 'single'" >&2; exit 2
  fi
  if [ "$mode" = "split" ] && [ -z "$contract" ]; then
    echo "error: --contract is required when --mode=split" >&2; exit 2
  fi
  if [ ${#tasks[@]} -eq 0 ]; then
    echo "error: --tasks must list at least one task ID" >&2; exit 2
  fi

  local senko
  senko="$(senko_bin)"

  local tmp
  tmp="$(mktemp)"
  {
    printf '# Mode: %s\n\n' "$mode"
    if [ "$mode" = "split" ]; then
      printf '# Contract\n\n'
      printf '## Contract %s\n\n' "$contract"
      printf '```json\n'
      "$senko" contract get "$contract"
      printf '\n```\n\n'
      printf '### Notes\n\n'
      printf '```json\n'
      "$senko" contract note list "$contract" --limit 200
      printf '\n```\n\n'
    fi
    printf '# Tasks\n\n'
    for tid in "${tasks[@]}"; do
      printf '## Task %s\n\n' "$tid"
      printf '```json\n'
      "$senko" task get "$tid"
      printf '\n```\n\n'
    done
  } > "$tmp"
  mv "$tmp" "$dir/packet.md"

  local tasks_json now meta meta_tmp
  tasks_json="$(printf '%s\n' "${tasks[@]}" | jq -R . | jq -s .)"
  now="$(now_iso)"
  meta="$dir/metadata.json"
  meta_tmp="$(mktemp)"
  jq --arg mode "$mode" \
     --arg contract "$contract" \
     --argjson tasks "$tasks_json" \
     --arg updated "$now" \
     '.mode = $mode
      | .contract_id = (if $contract == "" then null else $contract end)
      | .task_ids = $tasks
      | .updated_at = $updated' \
     "$meta" > "$meta_tmp"
  mv "$meta_tmp" "$meta"

  lazy_gc

  echo "$dir/packet.md"
}

usage() {
  cat <<'EOF'
Usage: senko-narrative.sh <subcommand> [args]

Subcommands:
  init                       Read {"intent":"..."} from stdin, print 8-char ID.
  append-decision <id>       Read {"q":"...","a":"..."} or {"note":"..."} from stdin.
  append-constraint <id>     Read {"text":"..."} from stdin.
  path <id>                  Print absolute path to narrative.md.
  packet-path <id>           Print absolute path to packet.md.
  build-packet <id> --mode {split|single} [--contract <cid>] --tasks <id1> [<id2> ...]
                             Build/overwrite packet.md and print its path.

Environment:
  SENKO_BIN              Path to senko binary (default: 'senko' in PATH).
  SENKO_STATE_DIR        State dir root (default: $XDG_STATE_HOME/senko).
  SENKO_GC_CUTOFF_DAYS   Lazy-GC cutoff in days (default: 7).
EOF
}

SUB="${1:-}"
if [ $# -gt 0 ]; then shift; fi

case "$SUB" in
  init) cmd_init "$@" ;;
  append-decision) cmd_append_decision "$@" ;;
  append-constraint) cmd_append_constraint "$@" ;;
  path) cmd_path "$@" ;;
  packet-path) cmd_packet_path "$@" ;;
  build-packet) cmd_build_packet "$@" ;;
  ""|-h|--help|help) usage; exit 0 ;;
  *)
    echo "error: unknown subcommand '$SUB'. Use --help for usage." >&2
    exit 2
    ;;
esac
