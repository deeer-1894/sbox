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
| Supply-chain integrity | unverified artifact => 0 executions | ToolService supply-chain-rejected count |
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
      - alert: SupplyChainRejection
        expr: increase(aep_tool_supplychain_rejected_total[5m]) > 0
        labels: { severity: critical }
      - alert: TenantQuotaSaturation
        expr: aep_tenant_inflight / aep_tenant_limit > 0.9
        for: 10m
        labels: { severity: warning }
```
