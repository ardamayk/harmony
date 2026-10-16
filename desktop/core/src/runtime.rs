//! The core's one tokio runtime. Background services (media proxy, Queue
//! events, MPRIS) and RPC handlers spawn onto it from any thread, including
//! the mpv event thread that publishes Playback Session updates.

use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("earthly-core")
            .build()
            .expect("the core's tokio runtime starts")
    })
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}

pub fn spawn_blocking<F, R>(function: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(function)
}

/// Runs a future to completion from a thread outside the runtime.
pub fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(future)
}
