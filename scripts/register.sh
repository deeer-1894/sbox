#!/usr/bin/env bash
# Register the host-run aep-runtime service endpoint with Restate.
# Run AFTER `cargo run -p aep-runtime` is listening on :9080.
set -euo pipefail
curl --fail --silent --show-error \
  http://localhost:9070/deployments \
  -H 'content-type: application/json' \
  -d '{"uri":"http://host.docker.internal:9080"}' | tee /dev/stderr
echo
echo "Registered. Services:"
curl --fail --silent http://localhost:9070/services | tee /dev/stderr
echo
