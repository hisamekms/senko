#!/usr/bin/env bash
set -euo pipefail

USER=dev
WORKSPACE="/workspaces/senko"

log() {
  printf '[post-create] %s\n' "$*"
}

on_error() {
  local exit_code=$?
  log "FAILED (exit=${exit_code}) at line ${BASH_LINENO[0]}: ${BASH_COMMAND}"
  exit "$exit_code"
}

trap on_error ERR

log "fix base ownership"
for dir in "$HOME/.local" "$HOME/.local/share" "$HOME/.cache" "$HOME/.config"; do
  sudo mkdir -p "$dir"
  sudo chown "$USER":"$USER" "$dir"
done

volume_paths=(
  "$HOME/.local/share/mise"
  "$HOME/.cache/prek"
  "$HOME/.claude"
  "$HOME/.cargo"
  "$HOME/.rustup"
)

log "prepare volume mounts"
for path in "${volume_paths[@]}"; do
  if [ -e "$path" ]; then
    sudo chown -R "$USER":"$USER" "$path"
  else
    sudo mkdir -p "$path"
    sudo chown -R "$USER":"$USER" "$path"
  fi
done

export PATH="$HOME/.local/bin:$PATH"

CHEZMOI_DIR="$HOME/.local/share/chezmoi"
if [ -d "$CHEZMOI_DIR" ] && [ -n "$(ls -A "$CHEZMOI_DIR" 2>/dev/null)" ]; then
  log "applying chezmoi"
  mise exec chezmoi -- chezmoi apply
  log "installing mise tools"
  mise install
else
  log "chezmoi source dir is empty or missing; skipping"
fi

if [ ! -f "$WORKSPACE/.env" ]; then
  if [ -f "$WORKSPACE/.env.example" ]; then
    log "initialize .env from .env.example"
    cp $WORKSPACE/.env.example $WORKSPACE/.env
  else
    log "no .env or .env.example found; skipping .env initialization"
  fi
else
  log ".env already exists; skipping initialization"
fi

BASH_ACTIVATE='eval "$(mise activate bash)"'
ZSH_ACTIVATE='eval "$(mise activate zsh)"'

if ! grep -qF 'mise activate bash' "$HOME/.bashrc" 2>/dev/null; then
  log "adding mise activate to .bashrc"
  echo "$BASH_ACTIVATE" >> "$HOME/.bashrc"
else
  log "mise activate already in .bashrc; skipping"
fi

if ! grep -qF 'mise activate zsh' "$HOME/.zshrc" 2>/dev/null; then
  log "adding mise activate to .zshrc"
  echo "$ZSH_ACTIVATE" >> "$HOME/.zshrc"
else
  log "mise activate already in .zshrc; skipping"
fi
