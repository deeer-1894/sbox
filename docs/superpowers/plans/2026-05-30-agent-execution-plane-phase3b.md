# Agent Execution Plane — Phase 3b (Process-Sandbox Tier) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide a Level-2 process-sandbox tier that runs untrusted code in an isolated child process which cannot reach the network — enforced by a seccomp filter, proven by the sandboxed code being denied a `socket()` syscall while ordinary code is not.

**Architecture:** A `aep-procsandbox` crate forks a child, installs `NO_NEW_PRIVS` + a seccomp BPF filter that returns `EPERM` for network syscalls, runs a closure, and returns its exit code. Linux-only; a stub on other platforms keeps the macOS workspace buildable. The real isolation is verified by running the crate's tests inside a Linux Docker container (`rust:1-slim-bookworm`).

**Tech Stack:** Rust, `libc`, `seccompiler` (Firecracker's pure-Rust seccomp compiler), `nix`-free raw `fork`/`waitpid`. Verified in a Linux container; the host is macOS. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3c.md`.

---

## Scope

Delivers the **process-sandbox tier (Level 2)** from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> Level 2: process sandbox with namespace/seccomp/cgroup ... POSIX-heavy tools: process sandbox in production.

This slice demonstrates the **seccomp** dimension (network lockdown), which is
unprivileged (works in a default container via `NO_NEW_PRIVS`). Namespace/cgroup
isolation requires privileges/host config and is layered on in production —
documented, not exercised here.

Verification runs in a Linux container because the host is macOS; the Docker
Desktop VM is Linux, so seccomp is real. `aep-procsandbox` is **not** wired into
`aep-runtime` (which runs on macOS and uses the Level-1 WASM sandbox); Level-2 is
a separate tier selected per tool class. Wiring is documented.

Prerequisite: Phase 3c complete and green.

## Key Design Decisions

- **Isolated child process.** The seccomp filter is irreversible and per-process,
  so the sandbox forks a child, locks it down, runs the closure, and `_exit`s —
  the parent is unaffected.
- **seccomp is the portable primitive.** It needs only `NO_NEW_PRIVS` (unprivileged),
  so it runs in a default Docker container without `--privileged`. Namespaces/cgroups
  need more privilege and are deferred.
- **Alloc-free child closures.** `fork()` in a multithreaded test process is only
  safe for async-signal-safe work; the demo closures do integer math / a raw
  `socket()` syscall and `_exit` (no heap, no destructors).
- **Linux-only, stub elsewhere.** `seccompiler`/`libc` are gated to
  `cfg(target_os = "linux")`; the macOS build compiles a stub so the workspace
  stays green on the dev host.

## File Structure

```
crates/
  aep-procsandbox/         NEW — fork + seccomp network lockdown (Linux), stub elsewhere
    Cargo.toml
    src/lib.rs
scripts/
  proc-sandbox-test.sh     NEW — run the Linux tests in a container
```

---

## Task 1: Process-sandbox crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-procsandbox/Cargo.toml`, `crates/aep-procsandbox/src/lib.rs`

- [ ] **Step 1: Add the workspace member**

In `Cargo.toml`, add `"crates/aep-procsandbox"` to `members` (before `aep-runtime`).

- [ ] **Step 2: Create the manifest (Linux-only deps)**

Create `crates/aep-procsandbox/Cargo.toml`:

```toml
[package]
name = "aep-procsandbox"
edition.workspace = true
version.workspace = true
license.workspace = true

[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
seccompiler = "0.4"
```

- [ ] **Step 3: Write the crate**

Create `crates/aep-procsandbox/src/lib.rs`:

```rust
//! Level-2 process sandbox: run a closure in an isolated child whose network
//! syscalls are denied by a seccomp filter. Linux-only; a stub elsewhere.

use std::io::{Error, ErrorKind, Result};

/// Run `f` in a forked child with a network-lockdown seccomp filter applied.
/// Returns the child's exit code. Linux-only.
#[cfg(target_os = "linux")]
pub fn run_isolated<F: FnOnce() -> i32>(f: F) -> Result<i32> {
    // SAFETY: the child runs an alloc-free closure and _exit; the parent only
    // waitpids. fork is the documented mechanism for an isolated sandbox process.
    match unsafe { libc::fork() } {
        -1 => Err(Error::last_os_error()),
        0 => {
            let code = match apply_network_lockdown() {
                Ok(()) => f(),
                Err(_) => 111,
            };
            unsafe { libc::_exit(code) }
        }
        pid => {
            let mut status: libc::c_int = 0;
            if unsafe { libc::waitpid(pid, &mut status, 0) } < 0 {
                return Err(Error::last_os_error());
            }
            if libc::WIFEXITED(status) {
                Ok(libc::WEXITSTATUS(status))
            } else {
                Err(Error::new(ErrorKind::Other, "sandboxed child did not exit normally"))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_network_lockdown() -> Result<()> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
    use std::collections::BTreeMap;

    #[cfg(target_arch = "aarch64")]
    const ARCH: seccompiler::TargetArch = seccompiler::TargetArch::aarch64;
    #[cfg(target_arch = "x86_64")]
    const ARCH: seccompiler::TargetArch = seccompiler::TargetArch::x86_64;

    // Required to install a seccomp filter unprivileged.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(Error::last_os_error());
    }

    // Deny socket creation (network) with EPERM; allow everything else.
    let rules = BTreeMap::from([(libc::SYS_socket, vec![])]);
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        ARCH,
    )
    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let prog: BpfProgram = filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| Error::new(ErrorKind::Other, e.to_string()))?;
    seccompiler::apply_filter(&prog).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    Ok(())
}

/// Non-Linux stub so the workspace builds on the dev host (macOS).
#[cfg(not(target_os = "linux"))]
pub fn run_isolated<F: FnOnce() -> i32>(_f: F) -> Result<i32> {
    Err(Error::new(ErrorKind::Unsupported, "process sandbox requires Linux"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn sandboxed_compute_runs() {
        assert_eq!(run_isolated(|| 2 + 3).unwrap(), 5);
    }

    #[test]
    fn socket_is_denied_in_sandbox() {
        let code = run_isolated(|| {
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                0 // denied (EPERM) — expected
            } else {
                unsafe { libc::close(fd) };
                1 // not denied — sandbox failed
            }
        })
        .unwrap();
        assert_eq!(code, 0, "socket() must be denied in the sandbox");
    }

    #[test]
    fn socket_works_without_sandbox() {
        // Contrast: the same syscall succeeds when not sandboxed.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        assert!(fd >= 0, "socket() works normally outside the sandbox");
        unsafe { libc::close(fd) };
    }
}
```

- [ ] **Step 4: Verify the macOS workspace still builds (stub)**

Run: `cargo build -p aep-procsandbox`
Expected: `Finished` (compiles the non-Linux stub; no `libc`/`seccompiler`).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-procsandbox
git commit -m "feat(procsandbox): seccomp network-lockdown process sandbox (Linux)"
```

---

## Task 2: Verify the sandbox in a Linux container

**Files:**
- Create: `scripts/proc-sandbox-test.sh`

- [ ] **Step 1: Write the container test script**

Create `scripts/proc-sandbox-test.sh`:

```bash
#!/usr/bin/env bash
# Verify the Level-2 process sandbox on Linux (the host is macOS; the Docker
# Desktop VM is Linux, so seccomp is real). A separate CARGO_TARGET_DIR keeps
# Linux artifacts out of the macOS target/.
set -euo pipefail
docker run --rm -v "$PWD":/work -w /work \
  -e CARGO_TARGET_DIR=/tmp/lt \
  rust:1-slim-bookworm \
  bash -lc 'cargo test -p aep-procsandbox -- --test-threads=1 --nocapture'
```

- [ ] **Step 2: Make it executable and run it**

Run: `chmod +x scripts/proc-sandbox-test.sh && ./scripts/proc-sandbox-test.sh`
Expected: 3 tests pass — `sandboxed_compute_runs`, `socket_is_denied_in_sandbox`, `socket_works_without_sandbox`.

> If linking fails for lack of a C compiler, prepend `apt-get update && apt-get install -y gcc &&` inside the container command. If `seccomp` is blocked, the container needs the default (not `--privileged=false` with a custom restrictive profile) seccomp profile — the default allows installing nested filters.

- [ ] **Step 3: Commit**

```bash
git add scripts/proc-sandbox-test.sh
git commit -m "test(procsandbox): Linux container verification of seccomp lockdown"
```

---

## Task 3: Acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3b-acceptance.md`

- [ ] **Step 1: Write acceptance**

Create the file:

```markdown
# Phase 3b (Process-Sandbox Tier) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 3, Level-2 sandbox)

Verified in a Linux container (rust:1-slim-bookworm) on a macOS host.

| Criterion | Evidence | Status |
| --- | --- | --- |
| Code runs in an isolated process | procsandbox::sandboxed_compute_runs (forked child returns 5) | [ ] |
| Sandboxed code cannot reach the network | procsandbox::socket_is_denied_in_sandbox (socket() => EPERM in sandbox) | [ ] |
| The denial is the sandbox, not the env | procsandbox::socket_works_without_sandbox (socket() succeeds unsandboxed) | [ ] |
| macOS workspace stays buildable | cargo build -p aep-procsandbox compiles the stub | [ ] |

Deferred / production: namespace + cgroup isolation (needs privilege/host config);
wiring Level-2 into ToolService for POSIX-heavy tool classes (the macOS runtime
uses Level-1 WASM); microVM (Level 3).
```

- [ ] **Step 2: Tick boxes after a clean run**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3b-acceptance.md
git commit -m "docs: Phase 3b acceptance checklist"
```

---

## Self-Review

**Spec coverage:** "process sandbox with seccomp" → Task 1 (`run_isolated` + `apply_network_lockdown`) + Task 2 (container verification). Namespace/cgroup deferred (privilege), documented. ✔

**Placeholder scan:** complete code; the seccompiler API (`SeccompFilter::new`, `try_into()`, `apply_filter`) is shown; container fallbacks (gcc, seccomp profile) noted. ✔

**Type consistency:** `run_isolated` signature identical across the Linux impl and the stub; tests call it with `FnOnce() -> i32` closures. `ARCH` cfg-selected per container arch. ✔

**Deferred:** namespace/cgroup tiers; microVM (Level 3); wiring Level-2 into ToolService per tool class.
