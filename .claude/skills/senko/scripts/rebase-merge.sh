#!/usr/bin/env bash
set -euo pipefail

BRANCH="${1:?Usage: rebase-merge.sh <branch-name>}"
LOCKFILE="/tmp/senko-rebase-merge.lock"

exec 200>"$LOCKFILE"
if ! flock -n 200; then
  echo "error: another rebase-merge is already running" >&2
  exit 1
fi

git checkout main
git pull --ff-only origin main 2>/dev/null || true
git rebase main "$BRANCH"
git checkout main
git merge --ff-only "$BRANCH"
