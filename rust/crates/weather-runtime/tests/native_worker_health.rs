//! Real worker health command output against the compiled predecessor's DTO serializer.

use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc};
use weather_runtime::{NativeWirePrimitives, native_worker_health_output, run_worker_health_check};
use weather_worker::{
    WorkerHealth, WorkerProcessFailure, WorkerProcessPool, WorkerProcessPoolFactory, worker_health,
};

const NOW: i64 = 1_790_000_000_000;

struct HealthPool {
    health: WorkerHealth,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl WorkerProcessPool<()> for HealthPool {
    /// Observe database readiness before reading durable health.
    fn assert_readiness(&mut self, _: &()) -> Result<(), WorkerProcessFailure> {
        self.events.borrow_mut().push("ready");
        Ok(())
    }

    /// Return the actual health projection derived from the fixture's durable timestamps.
    fn read_durable_health(&mut self, _: &()) -> Result<WorkerHealth, WorkerProcessFailure> {
        self.events.borrow_mut().push("read");
        Ok(self.health)
    }

    /// Record the source command's pool close.
    fn close(&mut self) -> Result<(), WorkerProcessFailure> {
        self.events.borrow_mut().push("close");
        Ok(())
    }
}

struct HealthFactory {
    health: WorkerHealth,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl WorkerProcessPoolFactory<()> for HealthFactory {
    type Pool = HealthPool;

    /// Create the actual command's one fixture-owned database lifecycle.
    fn create_pool(&mut self, _: &()) -> Result<Self::Pool, WorkerProcessFailure> {
        self.events.borrow_mut().push("create");
        Ok(HealthPool {
            health: self.health,
            events: Rc::clone(&self.events),
        })
    }
}

/// Prove the actual health command writes predecessor-identical lines before closing its pool.
#[test]
fn native_worker_health_command_writes_original_json_lines() {
    let actual = Command::new("node")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/worker-health-wire-node-oracle.mjs"),
        )
        .output()
        .unwrap();
    assert!(
        actual.status.success(),
        "{}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let mut output = Vec::new();
    // compare fresh, stale, missing and future heartbeat states on the real output path
    for (last_loop, last_success, ready) in [
        (Some(NOW - 60_000), Some(NOW - 120_000), true),
        (Some(NOW - 1_800_001), Some(NOW - 1_900_000), true),
        (None, None, false),
        (Some(NOW + 1), Some(NOW - 1_000), true),
    ] {
        let health = worker_health(NOW, last_loop, last_success, ready, 1_800_000);
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut factory = HealthFactory {
            health,
            events: Rc::clone(&events),
        };
        let mut writer = native_worker_health_output(&mut output, NativeWirePrimitives);
        let status = run_worker_health_check(&(), &mut factory, &mut writer).unwrap();
        assert_eq!(status, u8::from(!health.ready));
        assert_eq!(*events.borrow(), ["create", "ready", "read", "close"]);
    }
    assert_eq!(output, actual.stdout);
}
