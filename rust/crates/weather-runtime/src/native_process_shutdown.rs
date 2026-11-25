//! Shared process-level shutdown from the service manager's real Unix signals.

use std::{future::poll_fn, io, task::Poll};

use tokio::signal::unix::{Signal, SignalKind, signal};

/// Own both signal subscriptions before any service resource is started.
pub struct NativeProcessShutdown {
    interrupt: Signal,
    terminate: Signal,
}

impl NativeProcessShutdown {
    /// Register SIGINT and SIGTERM in the current native runtime.
    pub fn register() -> io::Result<Self> {
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    /// Wait for either source signal and let the process's retained owners drain.
    pub async fn wait(mut self) {
        poll_fn(|context| {
            // both receivers subscribe to the same runtime wakeup and finish only once
            if self.interrupt.poll_recv(context).is_ready()
                || self.terminate.poll_recv(context).is_ready()
            {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process::Command, time::Duration};

    use super::*;
    use crate::NativeBackendRuntime;

    /// Exercise both real Unix signals in isolated copies of the test process.
    #[test]
    fn process_shutdown_accepts_sigint_and_sigterm() {
        const CHILD_SIGNAL: &str = "WEATHER_TEST_NATIVE_SHUTDOWN_SIGNAL";
        // only the isolated child may change the process-wide signal subscriptions
        if let Ok(value) = env::var(CHILD_SIGNAL) {
            let signal = value.parse::<i32>().expect("test signal");
            let runtime = NativeBackendRuntime::new().expect("runtime");
            runtime.block_on(async {
                let shutdown = NativeProcessShutdown::register().expect("signals registered");
                // SAFETY: raise acts on this isolated child after both subscriptions exist.
                assert_eq!(unsafe { libc::raise(signal) }, 0);
                tokio::time::timeout(Duration::from_secs(2), shutdown.wait())
                    .await
                    .expect("process shutdown signal received");
            });
            return;
        }
        // distinguish the signals; each child exits after receiving only one
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let output = Command::new(env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "native_process_shutdown::tests::process_shutdown_accepts_sigint_and_sigterm",
                    "--nocapture",
                ])
                .env(CHILD_SIGNAL, signal.to_string())
                .output()
                .expect("isolated signal test");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}
