#!/usr/bin/env bash
# Crash-recovery check for the side-effect boundary.
#
# Procedure:
#   1. Ensure Restate is up and a deployment is registered (scripts/register.sh).
#   2. Start the service so its PID is known:
#        cargo run -p aep-runtime &  RUNTIME_PID=$!
#   3. Run this script. It fires one invocation, then kills the service mid-flight,
#      restarts it, re-registers, and reports the counter.
#
# Expected result: the counter advances by EXACTLY ONE across the whole episode.
# Restate retries the invocation after restart, but the committed ctx.run result
# is replayed from the journal — the external counter is not bumped a second time.
set -euo pipefail

KEY="recover-$(date +%s)"
BEFORE=$(curl --fail --silent http://localhost:9090/count)
echo "counter before: $BEFORE  key: $KEY"

# Fire the invocation in the background; do not wait for it.
curl --silent http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}" >/dev/null &

# Give it a moment to journal ToolRequested / run the side effect, then kill.
sleep 1
echo "killing aep-runtime (pkill); restart it manually, then re-run register.sh"
pkill -f 'target/debug/aep-runtime' || true

cat <<'EOF'

Now, in the service terminal:
  cargo run -p aep-runtime
Then re-register and read the counter:
  ./scripts/register.sh
  curl --silent http://localhost:9090/count ; echo

PASS if the counter equals (BEFORE + 1). FAIL if it is (BEFORE + 2).
EOF
