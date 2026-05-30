//! Wasmtime sandbox: no ambient authority; host access is capability-gated.

use aep_capability::{Action, Capability, Resource};
use thiserror::Error;
use wasmtime::{Config, Engine, Linker, Module, Store};

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("wasm load/instantiate error: {0}")]
    Load(String),
    #[error("wasm trap or fuel exhaustion: {0}")]
    Trap(String),
    #[error("missing export: {0}")]
    MissingExport(String),
}

const FUEL: u64 = 1_000_000;

fn engine() -> Engine {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).expect("valid wasmtime config")
}

/// The resource a tool's side-effect host function (`host_sink`) requires.
fn sink_resource() -> Resource {
    Resource::Tool { name: "sink".into() }
}

/// Run a pure-compute module's `add(i32,i32)->i32` export under a fuel bound and
/// an empty linker (no host authority whatsoever).
pub fn run_add(wat: &str, a: i32, b: i32) -> Result<i32, SandboxError> {
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let linker: Linker<()> = Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .map_err(|_| SandboxError::MissingExport("add".into()))?;
    f.call(&mut store, (a, b)).map_err(|e| SandboxError::Trap(e.to_string()))
}

/// Run a module that may import `env.host_sink(i32)->i32`. The host function is
/// linked ONLY when `cap` authorizes the sink resource. Returns the value the
/// module produces from its `run()->i32` export.
pub fn run_with_sink(wat: &str, cap: &Capability) -> Result<i32, SandboxError> {
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut linker: Linker<()> = Linker::new(&engine);

    if cap.authorize(Action::Call, &sink_resource()).is_ok() {
        linker
            .func_wrap("env", "host_sink", |arg: i32| -> i32 { arg })
            .map_err(|e| SandboxError::Load(e.to_string()))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|_| SandboxError::MissingExport("run".into()))?;
    f.call(&mut store, ()).map_err(|e| SandboxError::Trap(e.to_string()))
}

/// Run a tool module whose `run()->i32` export may call `env.host_sink()->i32`.
/// The host_sink is linked only when `cap` authorizes the sink resource; when
/// called, it invokes `sink` (the host side effect) and returns its value. The
/// closure is the host's authority — the WASM cannot reach it any other way.
pub fn run_tool<F>(wat: &str, cap: &Capability, sink: F) -> Result<i32, SandboxError>
where
    F: Fn() -> u64 + Send + Sync + 'static,
{
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut linker: Linker<()> = Linker::new(&engine);

    if cap.authorize(Action::Call, &sink_resource()).is_ok() {
        linker
            .func_wrap("env", "host_sink", move |_caller: wasmtime::Caller<'_, ()>| -> i32 {
                sink() as i32
            })
            .map_err(|e| SandboxError::Load(e.to_string()))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|_| SandboxError::MissingExport("run".into()))?;
    f.call(&mut store, ()).map_err(|e| SandboxError::Trap(e.to_string()))
}

#[cfg(test)]
mod compute_tests {
    use super::*;

    const ADD_WAT: &str = r#"
        (module
          (func (export "add") (param i32 i32) (result i32)
            local.get 0 local.get 1 i32.add))
    "#;

    #[test]
    fn runs_pure_compute() {
        assert_eq!(run_add(ADD_WAT, 2, 3).unwrap(), 5);
    }

    const SPIN_WAT: &str = r#"
        (module
          (func (export "add") (param i32 i32) (result i32)
            (loop $l br $l) unreachable))
    "#;

    #[test]
    fn fuel_bound_stops_runaway() {
        let err = run_add(SPIN_WAT, 1, 1).unwrap_err();
        assert!(matches!(err, SandboxError::Trap(_)), "got {err:?}");
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;

    const NEEDS_HOST_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (param i32) (result i32)))
          (func (export "add") (param i32 i32) (result i32)
            i32.const 0 call $sink))
    "#;

    #[test]
    fn module_importing_ungranted_host_fn_is_rejected() {
        let err = run_add(NEEDS_HOST_WAT, 1, 1).unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
    }
}

#[cfg(test)]
mod gated_tests {
    use super::*;

    const SINK_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (param i32) (result i32)))
          (func (export "run") (result i32)
            i32.const 7 call $sink))
    "#;

    fn cap_for(resource: Resource) -> Capability {
        Capability {
            id: "c".into(), tenant: "t".into(), subject: "s".into(),
            resource, actions: vec![Action::Call],
            expires_at: u64::MAX, policy_hash: "ph".into(), audit_id: "a".into(),
        }
    }

    #[test]
    fn granted_capability_allows_host_call() {
        let cap = cap_for(Resource::Tool { name: "sink".into() });
        let got = run_with_sink(SINK_WAT, &cap).unwrap();
        assert_eq!(got, 7, "host_sink echoes its argument when authorized");
    }

    #[test]
    fn ungranted_capability_omits_host_fn() {
        let cap = cap_for(Resource::Tool { name: "other".into() });
        let err = run_with_sink(SINK_WAT, &cap).unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
    }
}

#[cfg(test)]
mod effect_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    const RUN_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (result i32)))
          (func (export "run") (result i32) call $sink))
    "#;

    fn cap(resource: Resource) -> Capability {
        Capability {
            id: "c".into(), tenant: "t".into(), subject: "s".into(),
            resource, actions: vec![Action::Call],
            expires_at: u64::MAX, policy_hash: "ph".into(), audit_id: "a".into(),
        }
    }

    #[test]
    fn authorized_tool_runs_side_effect_once() {
        let counter = Arc::new(AtomicU64::new(0));
        let n = run_tool(RUN_WAT, &cap(Resource::Tool { name: "sink".into() }), {
            let c = counter.clone();
            move || c.fetch_add(1, Ordering::SeqCst) + 1
        })
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unauthorized_tool_cannot_run_side_effect() {
        let counter = Arc::new(AtomicU64::new(0));
        let err = run_tool(RUN_WAT, &cap(Resource::Tool { name: "other".into() }), {
            let c = counter.clone();
            move || c.fetch_add(1, Ordering::SeqCst) + 1
        })
        .unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
        assert_eq!(counter.load(Ordering::SeqCst), 0, "side effect must not run");
    }
}
