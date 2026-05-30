#!/usr/bin/env bash
# Deterministic crash-recovery check for the side-effect boundary.
#
# Proves: a tool side effect committed before a hard crash is recovered from
# Restate's journal (which lives in restate-server, not our process) and is NOT
# re-executed when the same invocation is resent after restart.
#
# Note on the counter: the Phase 0 "external" counter sidecar lives INSIDE the
# aep-runtime process, so `kill -9` resets it to 0 on restart. That is fine — it
# makes "no re-execution" visually obvious: if recovery worked, the post-crash
# resend reuses the journaled ToolCompleted (returns the original exec_count) and
# never POSTs the counter, so the fresh counter stays at 0. A production external
# effect (S3, an API) would instead persist via its own external_reference.
#
# Prereqs: Restate up + deployment registered; run from the repo root with the
# service started via `cargo run -p aep-runtime`.
set -euo pipefail

PID=$(pgrep -f 'target/debug/aep-runtime' | head -1)
KEY="recover-$(date +%s)"
echo "service pid=$PID  key=$KEY"

# 1. Invoke; the side effect runs once and is journaled by Restate.
R1=$(curl -s http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}")
EXEC1=$(echo "$R1" | grep -o '"exec_count":[0-9]*' | grep -o '[0-9]*')
echo "first invoke committed exec_count=$EXEC1"

# 2. Hard crash.
kill -9 "$PID"
sleep 1
echo "killed (kill -9)"

# 3. Restart the stateless service at the same URI (no re-register needed).
nohup cargo run -p aep-runtime >/tmp/aep-runtime.log 2>&1 &
disown
for i in $(seq 1 30); do curl -s http://localhost:9090/count >/dev/null 2>&1 && break; sleep 2; done
echo "restarted; fresh in-process counter=$(curl -s http://localhost:9090/count)"

# 4. Resend the same key; must reuse the journaled completion.
R2=$(curl -s http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}")
EXEC2=$(echo "$R2" | grep -o '"exec_count":[0-9]*' | grep -o '[0-9]*')
CNT=$(curl -s http://localhost:9090/count)

echo "=== RESULT ==="
if [ "$EXEC2" = "$EXEC1" ] && [ "$CNT" = "0" ]; then
  echo "PASS: resend after crash reused journaled completion (exec_count=$EXEC2) and did not re-run the side effect (fresh counter=$CNT)"
  exit 0
else
  echo "FAIL: EXEC1=$EXEC1 EXEC2=$EXEC2 counter=$CNT"
  exit 1
fi
