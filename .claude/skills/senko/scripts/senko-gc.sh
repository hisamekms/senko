#!/usr/bin/env bash
# Manual garbage collection for senko narrative state dirs.
#
# Usage: bash senko-gc.sh
#
# See lib/gc.sh for details.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/gc.sh
source "$SCRIPT_DIR/lib/gc.sh"

gc_run
