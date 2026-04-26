#!/usr/bin/env bash
# Garbage-collect senko narrative state dirs whose metadata.json is older than cutoff.
#
# Usage: source this file then call `gc_run`.
#
# Environment:
#   SENKO_STATE_DIR        — state dir root (default: $XDG_STATE_HOME/senko or ~/.local/state/senko)
#   SENKO_GC_CUTOFF_DAYS   — sweep entries older than this many days (default: 7)
#
# Behavior:
#   For each <state_dir>/<id>/metadata.json older than cutoff, rm -rf <state_dir>/<id>.
#   Skips entries that look invalid (no metadata.json) — they are left alone for diagnosis.
#   Idempotent and safe to run repeatedly.

senko_state_dir() {
  if [ -n "${SENKO_STATE_DIR:-}" ]; then
    echo "$SENKO_STATE_DIR"
  elif [ -n "${XDG_STATE_HOME:-}" ]; then
    echo "$XDG_STATE_HOME/senko"
  else
    echo "$HOME/.local/state/senko"
  fi
}

gc_run() {
  local state_dir
  state_dir="$(senko_state_dir)"
  [ -d "$state_dir" ] || return 0

  local cutoff_days="${SENKO_GC_CUTOFF_DAYS:-7}"
  case "$cutoff_days" in
    ''|*[!0-9]*) cutoff_days=7 ;;
  esac

  # find -mtime +N matches files modified more than (N+1)*24h ago, but it's
  # the standard knob; close enough for a 7-day default.
  local entry id_dir
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    id_dir="$(dirname "$entry")"
    rm -rf "$id_dir"
  done < <(find "$state_dir" -mindepth 2 -maxdepth 2 -name metadata.json -type f -mtime "+$cutoff_days" 2>/dev/null)
}
