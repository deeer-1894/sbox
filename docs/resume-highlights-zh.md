# Agent Execution Plane — 简历要点 / 面试讲点

> 一个用 Rust 构建的、面向长时运行 AI Agent 的**安全持久执行运行时**。单机端到端,
> 全部能力在真实 Restate / ClickHouse / Jaeger 栈上 live 验证。**全程 spec 驱动、测试先行。**

---

## 一句话描述(放在项目标题/开头)

- **最短**:用 Rust 在 Restate 之上构建了一个安全、可审计、防注入的 AI Agent 执行运行时。
- **一句话**:设计并实现了一个面向长时运行 AI Agent 的持久执行运行时,把 LLM 的不可信输出
  约束在「租户配额 → Cedar 策略 → 能力授权 → 供应链验签 → WASM 沙箱 → 输出净化 → 因果审计」
  的强制安全流水线里,单机端到端 live 验证。
- **核心理念(可当亮点讲)**:*buy the substrate, build the moat* —— 复用成熟的持久执行底座
  (Restate),把工程投入集中在别人没做好的「能力安全 + 沙箱 + 审计」层,而不是重造一个
  Temporal/Orleans。

---

## 简历要点(按角度选用,都可被代码/测试佐证)

**系统 / 后端方向**
- 在 Restate(Virtual Object)之上实现**effectively-once 副作用边界**:工具调用经持久 journal,
  `kill -9` + 重启后复用已提交结果、**不重复执行**;同一幂等键重发也不重跑——均有集成测试佐证。
- 强制 actor 单 turn 串行、确定性重放:把所有不确定性(时间、随机、外部调用)经 `ctx.run`
  journaled,保证崩溃恢复后状态一致(诊断并修复了把 `now()/uuid()` 直接写进 handler 的反模式)。
- **9 个 crate**的清晰边界:纯逻辑库零基础设施依赖(毫秒级单测)+ 薄运行时适配层接 Restate。

**安全方向**
- 设计了**能力(capability)安全模型**:短时、限定 scope 的 HMAC 签名 token + Cedar 策略即代码
  (默认拒绝),使 **LLM 输出(不可信意图)无法直接执行**——伪造能力在工具边界被拒、零副作用(已验证)。
- 实现 **WASM 工具沙箱**(Wasmtime):无环境主机权限,主机函数受能力门控;未授权能力连副作用都
  无法链接;fuel 限额杀死失控代码。
- 实现 **Level-2 进程沙箱**(seccomp 网络锁定 + `NO_NEW_PRIVS` + fork 隔离),在 Linux 容器中验证
  沙箱内 `socket()` 被拒、沙箱外正常。
- **供应链验签**:工具 WASM 构件执行前必须匹配 pinned 摘要(变异检验:改一字节即被拒)。
- **防记忆投毒**:工具输出归类为不可信、净化注入标记、带信任标签与来源溯源入库,可按信任级查询
  构成污染边界。

**可观测 / 数据方向**
- **因果审计**:事件按 `causal_parent` 链重建为完整因果链,可回答"哪条能力授权了这次副作用";
  双写 ClickHouse,用 SQL 做审计查询。
- 接入 **OpenTelemetry**:每个 actor turn 产出带 trace 属性的 span,经 OTLP 真导出到 Jaeger
  (踩并解决了 opentelemetry 0.27 SDK 的 API 版本对齐)。
- 多级**背压 / 配额**:per-tenant 并发配额,超限即快速拒绝。

---

## 技术栈

**Rust**(async/Tokio、trait 抽象、workspace 多 crate、`unsafe` FFI:`fork`/seccomp)·
**Restate**(durable execution / Virtual Objects / journaling / 幂等)·
**Wasmtime / WASI**(WASM 沙箱、host 函数、fuel)· **seccomp / seccompiler**(进程隔离)·
**Cedar**(策略即代码授权)· **OpenTelemetry / OTLP / Jaeger** · **ClickHouse** ·
**HMAC-SHA256**(能力 token)· **Docker Compose** · **TDD / clippy**

---

## 可量化事实(诚实、可核对)

- 11 个 crate(8 个纯逻辑库 + 进程沙箱 + 运行时 + 集成测试),5 个 Restate 服务 + sidecar。
- ~34 个单元测试 + 7 套集成测试 + 3 个 Linux 沙箱测试,**全部在真实栈上通过**;clippy 零警告。
- 9 个实现阶段,每个都有:设计 spec → 测试先行的 plan → 可核对的验收记录。
- 性能优化:消除每请求重复开销(Cedar 策略解析一次、HTTP 连接池复用、密钥/registry 一次构建)。

---

## 面试讲点(用来撑起上面每条 bullet)

- **为什么不自己造持久执行底座?** —— 能讲清 build-vs-buy:durable execution 是成熟商品
  (Temporal/Restate/Orleans),重造无差异化价值;护城河在安全+审计层。这体现工程判断力。
- **"模型不能直接执行" 具体怎么落地?** —— 模型输出只是 intent;策略(Cedar)独立判定、能力
  (capability)是唯一执行权威、沙箱强制、供应链验签构件。即使模型被攻陷或工具输出含注入,
  **也偷不到主机的 key/权限**(key 在能力门控的 host 函数另一侧)。
- **effectively-once 怎么保证?** —— Restate journaling + 幂等键 + `ctx.run` 包裹副作用 +
  外部对账规则(`ToolCompleted` 存在则复用,绝不因崩溃重放副作用)。
- **确定性重放的坑** —— 发现并修复了在 durable handler 里直接 `now()/uuid()` 导致重放不一致;
  改为经 `ctx` 注入的 journaled 值。
- **进程沙箱为什么用 seccomp 而非 namespace?** —— seccomp 配 `NO_NEW_PRIVS` 免特权,在默认
  Docker 容器即可工作;namespace/cgroup 需特权,留作生产。体现对取舍的理解。

---

## 诚实的范围与局限(**这是面试加分项,务必能讲清**)

- **单机**:未做分布式("planet scale":分片 actor directory、Raft、actor 迁移)。
- **工具是 demo**:用一个 echo WASM 演示链路;真实工具/真 LLM 是接入点,非已接入(架构已就位)。
- **部分基础设施为生产形态**:microVM(Level 3)、namespace/cgroup 完整隔离、签名构件 manifest
  (cosign)、批量 ClickHouse 写、跨服务 trace 传播——均已文档化为生产 follow-up,未实现。
- 能清楚区分"已验证 / 已设计未建 / 刻意不做",本身就是工程成熟度的体现。

---

## 我学到了什么 / 复盘(体现成长)

- 系统性理解了 **durable execution / 虚拟 actor** 的模型,以及它和"普通 Tokio 服务"的本质区别。
- 把**能力安全 + 沙箱**从概念做成可运行、可验证的代码,真切体会到"权限与模型输出分离"的价值。
- 实践了 **spec → plan → TDD → live 验证 → 回填发现** 的纪律:每个阶段先写设计与计划、查证真实
  外部 API(Restate/Cedar/Wasmtime/OTel/seccomp 的版本与签名),再小步提交。
- 若重来:更早把外部 API 版本钉死;更早引入跨服务 trace 上下文传播;工具执行做成可插拔的 tier
  选择(WASM / 进程 / microVM)而非硬编码。

---

## 怎么在简历里写(示例)

> **AI Agent 安全执行运行时 (Rust)** — 个人项目
> 在 Restate 之上构建面向长时运行 Agent 的持久执行运行时:effectively-once 副作用边界、
> 能力安全 + Cedar 策略使模型输出无法直接执行、Wasmtime/seccomp 双层沙箱、供应链验签、
> 注入净化、ClickHouse 因果审计 + OTLP/Jaeger 追踪。9 阶段 spec 驱动、测试先行,全部 live 验证。

仓库:`github.com/deeer-1894/sbox` · 设计文档与逐阶段验收见 `docs/superpowers/`。
