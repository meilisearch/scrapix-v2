//! Shared process-lifecycle helpers for Scrapix services on Fly.io.
//!
//! Three primitives, composed per service:
//! - [`install_signal_handlers`] — flips a shutdown flag on `SIGTERM` or Ctrl-C.
//!   Fly sends SIGTERM before SIGKILL; without a handler, in-flight Kafka
//!   messages lose their offset commit and get reprocessed.
//! - [`spawn_wake_listener`] — bare TCP listener on a configurable port that
//!   responds `200 OK` to any request. Fly's proxy uses an incoming TCP
//!   connection on a declared port to auto-start a suspended machine.
//! - [`spawn_idle_watchdog`] — watches a monotonically-increasing counter
//!   (typically `messages_processed`); flips the shutdown flag if it doesn't
//!   advance for `idle_minutes`. Lets Kafka-consumer workers exit cleanly
//!   when idle so Fly can suspend them to zero cost.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Install SIGTERM + Ctrl-C handlers that flip `shutdown` to `true` on the
/// first signal. Returns a `JoinHandle` you can drop; the task exits on its
/// own once a signal fires.
pub fn install_signal_handlers(shutdown: Arc<AtomicBool>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "Failed to register SIGTERM handler");
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT (Ctrl-C), initiating graceful shutdown");
            }
            _ = term.recv() => {
                info!("Received SIGTERM, initiating graceful shutdown");
            }
        }
        shutdown.store(true, Ordering::Relaxed);
    })
}

/// Spawn a bare-bones TCP listener that accepts connections on `port` and
/// responds `200 OK` to any payload. Exists solely so Fly.io's proxy sees
/// the port open and will autostart the machine on incoming connections
/// (e.g. from the API's worker-wake fan-out).
///
/// Exits when `shutdown` flips to `true`.
pub fn spawn_wake_listener(port: u16, shutdown: Arc<AtomicBool>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let bind_addr = format!("0.0.0.0:{}", port);
        let listener = match TcpListener::bind(&bind_addr).await {
            Ok(l) => l,
            Err(e) => {
                warn!(port, error = %e, "Wake listener failed to bind; Fly autostart will not work for this machine");
                return;
            }
        };
        info!(port, "Wake listener ready");

        loop {
            if shutdown.load(Ordering::Relaxed) {
                debug!("Wake listener shutting down");
                return;
            }
            let accept = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await;
            let (mut stream, _) = match accept {
                Ok(Ok(conn)) => conn,
                Ok(Err(e)) => {
                    debug!(error = %e, "Wake listener accept error");
                    continue;
                }
                Err(_) => continue, // timeout — loop and recheck shutdown
            };
            tokio::spawn(async move {
                const RESPONSE: &[u8] =
                    b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n";
                let _ = stream.write_all(RESPONSE).await;
                let _ = stream.shutdown().await;
            });
        }
    })
}

/// Watch a monotonically-increasing counter (typically messages processed)
/// and flip `shutdown` to `true` once it fails to advance for `idle_minutes`
/// of wall-clock time. `idle_minutes <= 0.0` disables the watchdog.
///
/// `sample_counter` is invoked periodically; it should return the current
/// cumulative count (e.g. `metrics.urls_processed.load(Ordering::Relaxed)`).
pub fn spawn_idle_watchdog<F>(
    sample_counter: F,
    idle_minutes: f64,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()>
where
    F: Fn() -> u64 + Send + Sync + 'static,
{
    tokio::spawn(async move {
        if idle_minutes <= 0.0 {
            debug!("Idle watchdog disabled (idle_minutes <= 0)");
            return;
        }
        let idle_duration = Duration::from_secs_f64(idle_minutes * 60.0);
        let sample_interval = Duration::from_secs_f64((idle_minutes * 60.0 / 6.0).max(10.0));

        let mut last_count = sample_counter();
        let mut last_change = tokio::time::Instant::now();
        info!(
            idle_minutes,
            sample_secs = sample_interval.as_secs(),
            "Idle watchdog started"
        );

        loop {
            tokio::time::sleep(sample_interval).await;
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            let current = sample_counter();
            if current != last_count {
                last_count = current;
                last_change = tokio::time::Instant::now();
                continue;
            }
            if last_change.elapsed() >= idle_duration {
                info!(
                    idle_minutes,
                    processed = current,
                    "Idle watchdog: no work for configured window, triggering shutdown"
                );
                shutdown.store(true, Ordering::Relaxed);
                return;
            }
        }
    })
}

/// Convenience: read `IDLE_EXIT_MINUTES` env var, defaulting to `default_minutes`.
/// Set to `0` to disable. Invalid values fall back to the default with a warning.
pub fn idle_minutes_from_env(default_minutes: f64) -> f64 {
    match std::env::var("IDLE_EXIT_MINUTES") {
        Ok(s) => s.parse::<f64>().unwrap_or_else(|_| {
            warn!(
                value = %s,
                "IDLE_EXIT_MINUTES is not a valid number, using default"
            );
            default_minutes
        }),
        Err(_) => default_minutes,
    }
}

/// Convenience: read `WAKE_PORT` env var, defaulting to `8081`.
pub fn wake_port_from_env() -> u16 {
    std::env::var("WAKE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8081)
}
