# Rust Actor OS Runtime MVP Verification

## Commands

- `cargo check --workspace`
- `cargo test --workspace`

## Expected MVP Properties

- Actor event log rejects non-contiguous sequence numbers.
- Actor mailbox deduplicates idempotency keys and enforces capacity.
- ActorRunner pulls bounded messages and commits events.
- CapabilityBroker authorizes only scoped tool calls.
- FakeSandbox refuses execution without a matching capability.
- AgentActor emits ToolIntent from UserInput.
- ToolActor records ToolRequested and emits RunTool.
- Integration flow is replayable by actor ID and sequence range.

## Known Non-MVP Areas

- No persistent WAL backend.
- No distributed scheduler.
- No process or microVM sandbox.
- No production Wasmtime component execution.
- No actor migration.
