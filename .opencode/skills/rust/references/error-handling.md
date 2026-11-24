# Explicit errors in Rust

## Layered model

| Layer | Error type | Tool |
|---|---|---|
| Domain / library | Closed error enum, variants with context | `thiserror` |
| Adapter (DB, HTTP, queue) | Adapter error + `#[from]`/`#[source]` | `thiserror` |
| Application / binary | Aggregation with readable context | `anyhow` (here only) |
| HTTP/CLI boundary | Mapping to status + stable generic message | `IntoResponse` / `main` |

`anyhow` must not appear in a library's public API: it erases the information the caller needs to
decide. In the binary it is acceptable, but the final error shown to the user must not include the
cause chain.

## Canonical pattern

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("resource not found")]
    NotFound,
    #[error("operation not permitted")]
    Forbidden,
    #[error("invalid input: {field}")]
    InvalidInput { field: &'static str },
    #[error("version conflict")]
    Conflict,
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DomainError>;
```

Rules:

- User-facing variants carry no internal details (SQL, paths, hosts, versions).
- `#[source]` preserves the cause for logs; `Display` does not print it.
- One `From` per boundary; avoid implicit conversion chains that hide the origin.
- Validation errors group all fields, not just the first, when UX requires it.

## HTTP boundary (axum)

```rust
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        let (status, code) = match err {
            DomainError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            DomainError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            DomainError::InvalidInput { .. } => (StatusCode::BAD_REQUEST, "invalid_input"),
            DomainError::Conflict => (StatusCode::CONFLICT, "conflict"),
            DomainError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        // Full log with request_id; the client only sees `code`.
        if status.is_server_error() {
            tracing::error!(error = ?err, "request failed");
        }
        Self { status, code }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body { code: &'static str }
        (self.status, Json(Body { code: self.code })).into_response()
    }
}
```

- The full detail goes to the log correlated by `request_id`; never to the body.
- Stable, documented messages and codes; do not change the contract for convenience.
- For 5xx, do not reveal whether the DB, the network, or an invariant failed.

## Concrete prohibitions

```rust
// Bad: loses the error and may leave partial state
let _ = tx.send(msg).await;

// Good: propagate or decide explicitly
tx.send(msg).await.map_err(|_| DomainError::Internal(anyhow!("worker channel closed")))?;
```

```rust
// Bad: panic in a production path
let row = rows.first().unwrap();

// Good
let row = rows.first().ok_or(DomainError::NotFound)?;
```

```rust
// Bad: indexing that can panic
let byte = buf[i];

// Good
let byte = *buf.get(i).ok_or(DomainError::InvalidInput { field: "offset" })?;
```

- `unwrap_or`, `unwrap_or_else`, `unwrap_or_default` are acceptable: they do not panic.
- `expect` at startup (`main`) only if the failure is unrecoverable and the message explains why.
- In tests, `unwrap()`/`expect()` are allowed; relax lints per module, not for the whole crate.

## Errors in `Drop`, tasks, and callbacks

- `Drop` cannot return `Result`: log the failure with `tracing::error!` and continue.
- In `tokio::spawn` tasks, the `JoinHandle` returns `Result<T, JoinError>`: handle the `JoinError`
  (panic or cancellation); do not discard it.
- Callbacks required by libraries to return `()` must turn the error into a log plus a safe state.

## Error tests

Every relevant variant needs a test proving the negative path:

```rust
#[tokio::test]
async fn missing_user_returns_404_without_leaking_details() {
    let res = app.oneshot(get("/users/does-not-exist")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

Cover: invalid input, permission denied, another tenant's resource, conflict, limit exceeded,
timeout, and a failing external dependency. Assert that the body contains no paths, SQL, hosts, or
versions.
