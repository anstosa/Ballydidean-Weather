//! One current-thread native process runtime with both required drivers enabled.

use std::{future::Future, io};

use tokio::{
    runtime::{Builder, Runtime},
    task::LocalSet,
};

/// Own the one I/O and timer driver and the local tasks admitted by a backend process.
pub struct NativeBackendRuntime {
    local: LocalSet,
    runtime: Runtime,
}

impl NativeBackendRuntime {
    /// Construct the actual process runtime with both network and time drivers available.
    pub fn new() -> io::Result<Self> {
        let runtime = Builder::new_current_thread().enable_all().build()?;
        Ok(Self {
            local: LocalSet::new(),
            runtime,
        })
    }

    /// Drive the process's listener and timer futures in its owned local runtime.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.local.block_on(&self.runtime, future)
    }
}
