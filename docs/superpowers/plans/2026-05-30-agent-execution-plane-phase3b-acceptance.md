# Phase 3b (Process-Sandbox Tier) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 3, Level-2 sandbox)

Verified 2026-05-31 in a Linux container (`rust:1-slim-bookworm`, aarch64) on a
macOS host — the Docker Desktop VM is Linux, so the seccomp filter is real.

| Criterion | Evidence | Status |
| --- | --- | --- |
| Code runs in an isolated process | `procsandbox::sandboxed_compute_runs`: the forked, locked-down child returns 5 | [x] |
| Sandboxed code cannot reach the network | `procsandbox::socket_is_denied_in_sandbox`: `socket()` returns EPERM inside the seccomp sandbox | [x] |
| The denial is the sandbox, not the env | `procsandbox::socket_works_without_sandbox`: `socket()` succeeds when not sandboxed | [x] |
| macOS workspace stays buildable | `cargo build -p aep-procsandbox` compiles the non-Linux stub | [x] |

Run it: `./scripts/proc-sandbox-test.sh` (3 tests pass).

## Mechanism

A forked child sets `PR_SET_NO_NEW_PRIVS` (unprivileged) and installs a
`seccompiler` BPF filter returning `EPERM` for the `socket` syscall, then runs
the closure and `_exit`s. seccomp needs no `--privileged` (Docker's default
profile permits installing nested filters), so it runs in an ordinary container.

## Deferred / production

- Namespace (PID/mount/net/user) + cgroup isolation — needs privilege/host config.
- Wiring Level-2 into ToolService for POSIX-heavy tool classes (the macOS runtime
  uses the Level-1 WASM sandbox; `aep-procsandbox` is a separate, Linux-only tier).
- microVM (Level 3) via Firecracker.

This completes every phase exercisable from this environment: Phase 0, 1, 1b, 2a,
2b, 2c, 3a, 3b, 3c.
