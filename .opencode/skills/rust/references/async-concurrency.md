# Async, concurrency, and the runtime

## Base rule

An async runtime is not an elastic pool: every task that blocks a worker delays all the others.
Anything that is not cooperative `.await` must leave the runtime thread or be justified.

| Operation | Treatment |
|---|---|
| `std::fs`, `std::net` | `tokio::fs`, `tokio::net`, or `spawn_blocking` |
| Hashing/compression/large parsing | `spawn_blocking` + size limit |
| DB client with a blocking driver | Dedicated pool or `spawn_blocking` |
| `std::thread::sleep` | `tokio::time::sleep` |
| CPU-intensive loop | Chunk with `yield_now().await` or `spawn_blocking` |
| Library with no async version | Wrap in `spawn_blocking`; never call directly |

```rust
// Bad
let data = std::fs::read(path)?;

// Good
let data = tokio::fs::read(path).await?;

// Heavy CPU with a typed return
let digest = tokio::task::spawn_blocking(move || compute_digest(&buf))
    .await
    .map_err(|e| DomainError::Internal(e.into()))??;
```

- `spawn_blocking` uses a separate, bounded pool; do not saturate it with hours-long tasks.
- Dropping a `spawn_blocking` handle does not cancel the work; design for idempotency.
- Never call `Runtime::block_on` inside an async task: it can block or panic.

## Locks and shared state

- `std::sync::Mutex` is valid if the lock does not cross an `.await` and the section is short.
- If it must cross an `await`, use `tokio::sync::Mutex` and justify why (holding the lock while
  waiting usually signals a better design).
- Avoid `RwLock` as a premature optimization: measure. Use `parking_lot` only if it helps and is
  audited.
- `static mut` is forbidden. Use `OnceLock`/`LazyLock` for initialization and `Arc<...>` for
  sharing.
- `RefCell`/`Cell` only in single-threaded contexts with no await in between; not in handlers.
- The `clippy::await_holding_lock` lint must be enabled and set to `deny`.

```rust
// Good: short lock, no await inside
let value = {
    let guard = state.read().expect("lock poisoned");
    guard.value.clone()
};
process(value).await?;
```

## Channels, backpressure, and bounded concurrency

```rust
use tokio::sync::mpsc;

// Bounded channel: the producer waits if the consumer cannot keep up.
let (tx, mut rx) = mpsc::channel::<Job>(64);

// Bounded concurrency
let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
for job in jobs {
    let permit = semaphore.clone().acquire_owned().await?;
    tokio::spawn(async move {
        let _permit = permit;
        handle(job).await
    });
}
```

- `unbounded_channel` is a memory bomb: only with intrinsically limited producers and documented.
- Use `JoinSet` to await and propagate child task errors:

```rust
let mut set = tokio::task::JoinSet::new();
for job in jobs {
    set.spawn(handle(job));
}
while let Some(res) = set.join_next().await {
    res.map_err(|e| DomainError::Internal(e.into()))??;
}
```

- Define what happens if a child task fails: cancel the rest (`abort_all`), retry, or aggregate
  errors. Do not leave it implicit.
- Measure in-flight work and apply limits to memory too (batch size, bytes per request).

## Timeouts, cancellation, and retries

```rust
use std::time::Duration;
use tokio::time::timeout;

let res = timeout(Duration::from_secs(5), client.get(url).send())
    .await
    .map_err(|_| DomainError::Internal(anyhow!("upstream timeout")))??;
```

- Every external I/O needs connect, read, and overall timeouts. Without them, a slow upstream
  exhausts your resources.
- Cancellation is cooperative: a `select!` that drops a branch does not revert its effects. Use
  guards and idempotent operations.
- `CancellationToken` for graceful shutdown:

```rust
let token = CancellationToken::new();
tokio::select! {
    _ = token.cancelled() => { /* cleanup */ }
    res = work() => res?,
}
```

- Retry only transient errors, with exponential backoff + jitter, a maximum budget, and never retry
  non-idempotent operations without an idempotency key.
- Avoid `tokio::spawn` for work that must finish before responding; await the future directly or use
  a durable queue.

## Graceful shutdown

- Listen for signals (`tokio::signal::ctrl_c`, SIGTERM) and stop accepting new work.
- Stop accepting connections, wait for in-flight tasks with a deadline, close the pool, and only
  then force `abort`.
- Do not lose in-flight messages: drain channels or persist before exiting.

## Async tests

- `#[tokio::test]` with a multi-thread runtime only when testing real concurrency.
- Use `tokio::time::pause()`/`advance()` for timeouts without real waits.
- Test cancellation: abort a task midway and verify the state remains consistent.
- Test saturation: fill the channel/semaphore and confirm backpressure, not unbounded growth.
