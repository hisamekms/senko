#!/bin/sh
set -euo pipefail

REPO="hisamekms/localflow"
INSTALL_DIR="${LOCALFLOW_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Linux)
    case "${ARCH}" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *)
        echo "Error: unsupported architecture: ${ARCH}" >&2
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "${ARCH}" in
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *)
        echo "Error: unsupported architecture: ${ARCH} (only Apple Silicon is supported)" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Error: unsupported OS: ${OS}" >&2
    exit 1
    ;;
esac

# Determine version
if [ -n "${VERSION:-}" ]; then
  TAG="${VERSION}"
else
  echo "Fetching latest release..."
  TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  if [ -z "${TAG}" ]; then
    echo "Error: could not determine latest release" >&2
    exit 1
  fi
fi

echo "Installing localflow ${TAG} for ${TARGET}..."

# Download and extract
ASSET="localflow-${TAG}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

curl -fsSL "${URL}" -o "${TMPDIR}/${ASSET}"
tar xzf "${TMPDIR}/${ASSET}" -C "${TMPDIR}"

# Install
mkdir -p "${INSTALL_DIR}"
install -m 755 "${TMPDIR}/localflow" "${INSTALL_DIR}/localflow"

echo "Installed localflow to ${INSTALL_DIR}/localflow"

# Check PATH
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Warning: ${INSTALL_DIR} is not in your PATH."
    echo "Add it with:  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
