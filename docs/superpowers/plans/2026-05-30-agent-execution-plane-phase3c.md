# Agent Execution Plane — Phase 3c (Supply-Chain Verification & SLOs) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A tool's WASM artifact is verified against a pinned trusted digest before the sandbox executes it — an unaudited or tampered artifact is rejected, never run. Plus documented SLOs with their metric sources.

**Architecture:** A pure `aep-supplychain` crate computes artifact digests and verifies them against a trusted `Registry` (unit-tested for accept/tamper/unknown). `ToolService` verifies the tool artifact against a pinned digest before invoking the sandbox. SLO targets and their metric sources (derived from the Phase 2c spans) are documented.

**Tech Stack:** Rust, `sha2`, the Phase 0–3a `restate-sdk = "0.8"` runtime. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3a.md`.

---

## Scope

Delivers the **supply-chain verification** slice of Phase 3 from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> supply-chain verification is required before production plugin execution.

SLO dashboards are **documented** (`deploy/slo/README.md`): targets + the metric
sources (the Phase 2c actor-turn spans / structured events). Live Grafana/Prometheus
is not runnable here (image registry rate-limited, as with ClickHouse/Jaeger).

Deferred: **Phase 3b** process-sandbox tier (Linux namespaces/seccomp/cgroups —
not exercisable on macOS).

Prerequisite: Phase 3a complete and green.

## Key Design Decisions

- **Verify before execute.** `ToolService` digests the tool artifact and compares
  it to a **pinned trusted digest** before calling the sandbox. A mismatch (or an
  unregistered tool) is a terminal rejection — the side effect never runs.
- **Pinned digest is an independent declaration.** `TRUSTED_ECHO_DIGEST` is a
  const (computed from the audited artifact and committed). If the embedded WASM
  changes without re-pinning, verification fails — that is the integrity guarantee.
  (Production loads a signed manifest instead of a const.)
- **Verification logic is pure + tested.** `aep-supplychain` proves accept,
  digest-mismatch, and unknown-artifact paths with no infrastructure.
- **SLOs ride existing telemetry.** Targets are documented against the Phase 2c
  actor-turn events; no new runtime surface needed for this slice.

## File Structure

```
crates/
  aep-supplychain/         NEW — sha256 digest + trusted Registry + verify (unit tested)
    Cargo.toml
    src/lib.rs
  aep-runtime/             MODIFY — ToolService verifies the tool artifact before sandbox
    src/lib.rs
deploy/slo/
  README.md                NEW — SLO targets + metric sources (documented)
```

---

## Task 1: Supply-chain verification crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-supplychain/Cargo.toml`, `crates/aep-supplychain/src/lib.rs`

- [ ] **Step 1: Add the workspace member**

In `Cargo.toml`, add `"crates/aep-supplychain"` to `members` (before `aep-runtime`).

- [ ] **Step 2: Create the manifest**

Create `crates/aep-supplychain/Cargo.toml`:

```toml
[package]
name = "aep-supplychain"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
sha2 = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 3: Write the lib with failing tests**

Create `crates/aep-supplychain/src/lib.rs`:

```rust
//! Supply-chain verification: tool artifacts must match a pinned trusted digest.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

/// SHA-256 hex digest of an artifact's bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SupplyChainError {
    #[error("artifact '{0}' is not in the trusted registry")]
    UnknownArtifact(String),
    #[error("artifact '{name}' digest mismatch (untrusted/tampered)")]
    DigestMismatch { name: String },
}

/// A registry of trusted artifact digests (the pinned manifest).
#[derive(Default)]
pub struct Registry {
    trusted: HashMap<String, String>,
}

impl Registry {
    /// Pin a trusted digest for a named artifact.
    pub fn register(&mut self, name: &str, digest: &str) {
        self.trusted.insert(name.to_string(), digest.to_string());
    }

    /// Verify `artifact` bytes match the pinned digest for `name`.
    pub fn verify(&self, name: &str, artifact: &[u8]) -> Result<(), SupplyChainError> {
        let expected = self
            .trusted
            .get(name)
            .ok_or_else(|| SupplyChainError::UnknownArtifact(name.to_string()))?;
        if &sha256_hex(artifact) != expected {
            return Err(SupplyChainError::DigestMismatch { name: name.to_string() });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_pinned_artifact() {
        let mut reg = Registry::default();
        reg.register("echo", &sha256_hex(b"wasm-bytes"));
        assert_eq!(reg.verify("echo", b"wasm-bytes"), Ok(()));
    }

    #[test]
    fn rejects_tampered_artifact() {
        let mut reg = Registry::default();
        reg.register("echo", &sha256_hex(b"wasm-bytes"));
        assert_eq!(
            reg.verify("echo", b"tampered-bytes"),
            Err(SupplyChainError::DigestMismatch { name: "echo".into() })
        );
    }

    #[test]
    fn rejects_unknown_artifact() {
        let reg = Registry::default();
        assert_eq!(
            reg.verify("ghost", b"x"),
            Err(SupplyChainError::UnknownArtifact("ghost".into()))
        );
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aep-supplychain`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-supplychain
git commit -m "feat(supplychain): artifact digest + trusted registry verification"
```

---

## Task 2: ToolService verifies the artifact before the sandbox

**Files:**
- Modify: `crates/aep-runtime/Cargo.toml`
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `crates/aep-runtime/Cargo.toml` `[dependencies]`, add:

```toml
aep-supplychain = { path = "../aep-supplychain" }
```

- [ ] **Step 2: Hoist TOOL_WAT to a module-level const**

In `crates/aep-runtime/src/lib.rs`, the `TOOL_WAT` const currently lives inside `ToolService::run`'s `Decision::Execute` arm. Move it to a file-level const (near the top, after `COUNTER_BASE`) and delete the inner `const TOOL_WAT` line so the Execute arm references the module-level one:

```rust
/// The audited tool WASM (WAT form). Its digest is pinned in TRUSTED_ECHO_DIGEST.
const TOOL_WAT: &str = r#"
                    (module
                      (import "env" "host_sink" (func $sink (result i32)))
                      (func (export "run") (result i32) call $sink))
                "#;
```

- [ ] **Step 3: Add the pinned digest and verify before the sandbox**

In `ToolService::run`, after the capability `authorize` and the Phase 2c span
record (before `// --- Phase 0 side-effect boundary`), add the supply-chain gate.
The `TRUSTED_ECHO_DIGEST` value is computed in Step 4 — use a placeholder for now:

```rust
        // Supply-chain: the tool artifact must match the pinned trusted digest
        // before the sandbox runs it. (Production loads a signed manifest.)
        let mut registry = aep_supplychain::Registry::default();
        registry.register("echo-tool", TRUSTED_ECHO_DIGEST);
        registry
            .verify("echo-tool", TOOL_WAT.as_bytes())
            .map_err(|e| TerminalError::new(format!("supply-chain verification failed: {e}")))?;
```

And add the const near `TOOL_WAT` (value filled in Step 4):

```rust
/// Pinned SHA-256 of the audited TOOL_WAT. Regenerate if the artifact changes:
///   the value equals aep_supplychain::sha256_hex(TOOL_WAT.as_bytes()).
const TRUSTED_ECHO_DIGEST: &str = "PLACEHOLDER";
```

- [ ] **Step 4: Compute and pin the real digest**

Add a temporary test to print the digest, run it, then paste the value into
`TRUSTED_ECHO_DIGEST` and remove the temp test:

```rust
// temporary — delete after pinning
#[cfg(test)]
mod _pin {
    #[test]
    fn print_digest() {
        println!("DIGEST={}", aep_supplychain::sha256_hex(super::TOOL_WAT.as_bytes()));
    }
}
```

Run: `cargo test -p aep-runtime _pin::print_digest -- --nocapture | grep DIGEST`
Copy the hex into `TRUSTED_ECHO_DIGEST`, then delete the `_pin` module.

- [ ] **Step 5: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 6: Commit**

```bash
git add crates/aep-runtime/Cargo.toml crates/aep-runtime/src/lib.rs
git commit -m "feat(runtime): ToolService verifies tool artifact digest before sandbox"
```

---

## Task 3: SLO documentation

**Files:**
- Create: `deploy/slo/README.md`

- [ ] **Step 1: Write the SLOs**

Create `deploy/slo/README.md`:

```markdown
# Service Level Objectives (SLOs)

SLOs ride the Phase 2c instrumentation: every actor turn emits a structured
`tracing` event (`trace_id`, `actor`, `kind`). A metrics pipeline (OTLP →
collector → Prometheus, see deploy/otel/README.md) derives the series below.
Grafana/Prometheus are not run here (registry images rate-limited); this file is
the SLO definition.

## Objectives

| SLO | Target | Source signal |
| --- | --- | --- |
| Agent request availability | 99.9% non-5xx over 30d | count of AgentService.handle turns vs error events |
| Tool side-effect effectively-once | 100% (zero double-execution) | ToolService journaled ToolCompleted reuse rate |
| Policy-deny correctness | denied requests perform 0 side effects | audit chain: policy_deny without tool_requested |
| Capability rejection | forged/expired capability => 0 executions | ToolService capability-rejected count |
| Tenant fairness | per-tenant in-flight <= configured limit | TenantService acquire/reject ratio |
| Recovery time | < 5s p99 after a node restart | Restate invocation resume latency |

## Alert rules (Prometheus, illustrative)

```yaml
groups:
  - name: aep
    rules:
      - alert: ToolDoubleExecution
        expr: increase(aep_tool_side_effect_duplicate_total[5m]) > 0
        labels: { severity: critical }
      - alert: TenantQuotaSaturation
        expr: aep_tenant_inflight / aep_tenant_limit > 0.9
        for: 10m
        labels: { severity: warning }
```
```

- [ ] **Step 2: Commit**

```bash
git add deploy/slo/README.md
git commit -m "docs: SLO targets, metric sources, and alert rules"
```

---

## Task 4: Supply-chain regression on the live path

The tool path (sandbox_chain, memory, audit, telemetry tests) now runs through
the supply-chain gate. A passing regression proves the gate is on the path and
admits the audited artifact; the rejection paths are unit-tested in Task 1.

- [ ] **Step 1: Restart, re-register, full regression**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest -- --ignored
```
Expected: all integration tests pass (effectively_once, security_chain×3, sandbox_chain, quota×3, audit, telemetry, memory). If the tool no longer runs, the pinned digest is wrong (re-pin via Task 2 Step 4).

- [ ] **Step 2: Prove the gate has teeth (mutation check)**

Temporarily change one character inside `TOOL_WAT` (e.g., a comment), rebuild,
restart, re-register, and run `cargo test -p aep-itest --test sandbox_chain -- --ignored`.
Expected: the tool invocation fails (supply-chain verification failed) because the
digest no longer matches the pinned value. Then **revert** the change, rebuild,
restart, re-register, and confirm the test passes again.

- [ ] **Step 3: Commit (no code change; evidence only)**

No commit needed for the mutation check (reverted). Proceed to acceptance.

---

## Task 5: Acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3c-acceptance.md`

- [ ] **Step 1: Write acceptance**

Create the file:

```markdown
# Phase 3c (Supply-Chain Verification & SLOs) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 3, supply-chain + SLO)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Tool artifact verified before execution | ToolService verifies TOOL_WAT against TRUSTED_ECHO_DIGEST before the sandbox; mutation check (changed WAT) fails the tool invocation | [ ] |
| Tampered/unknown artifact rejected | aep-supplychain tests: digest mismatch + unknown artifact rejected | [ ] |
| Verification pure + tested | aep-supplychain accept/tamper/unknown tests pass | [ ] |
| SLOs defined with metric sources | deploy/slo/README.md (targets + source signals + alert rules) | [ ] |
| No earlier-phase regression | full cargo test -p aep-itest -- --ignored passes | [ ] |

Deferred: Phase 3b process-sandbox tier (Linux); live Grafana/Prometheus SLO
dashboards (registry images rate-limited). Production: signed artifact manifest
(replacing the pinned const), sigstore/cosign verification.
```

- [ ] **Step 2: Tick boxes after a clean run**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3c-acceptance.md
git commit -m "docs: Phase 3c acceptance checklist"
```

---

## Self-Review

**Spec coverage:** "supply-chain verification before production plugin execution" → Task 1 (`Registry::verify`) + Task 2 (ToolService gate before sandbox) + Task 4 (mutation check proves teeth). "SLO dashboards" → Task 3 documented targets + metric sources (live Grafana deferred on infra grounds). ✔

**Placeholder scan:** the only placeholder (`TRUSTED_ECHO_DIGEST = "PLACEHOLDER"`) is explicitly resolved in Task 2 Step 4 with a computation step. No other TBDs. ✔

**Type consistency:** `sha256_hex`/`Registry`/`SupplyChainError` defined in Task 1, used in Task 2. `TOOL_WAT` hoisted in Task 2 Step 2 is the same const the Execute arm uses. `TRUSTED_ECHO_DIGEST` defined and consumed in Task 2. ✔

**Deferred:** Phase 3b (Linux process sandbox); production signed manifests + cosign; live SLO dashboards. This is the last slice executable on the macOS host — Phase 3b requires a Linux target.
