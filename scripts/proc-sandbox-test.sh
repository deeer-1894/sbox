#!/usr/bin/env bash
# Verify the Level-2 process sandbox on Linux (the host is macOS; the Docker
# Desktop VM is Linux, so seccomp is real). A separate CARGO_TARGET_DIR keeps
# Linux artifacts out of the macOS target/. cargo is on the image's PATH, so do
# not wrap in a login shell (which resets PATH).
set -euo pipefail
docker run --rm -v "$PWD":/work -w /work \
  -e CARGO_TARGET_DIR=/tmp/lt \
  rust:1-slim-bookworm \
  cargo test -p aep-procsandbox -- --test-threads=1 --nocapture
