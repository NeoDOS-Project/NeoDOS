# Rust web services (axum/tower)

This document assumes axum + tower/tower-http, the most common stack. The same principles apply to
actix-web.

## Limits at the boundary

```rust
use axum::{extract::DefaultBodyLimit, Router};

let app = Router::new()
    .route("/upload", axum::routing::post(upload))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
    .layer(tower_http::limit::RequestBodyLimitLayer::new(2 * 1024 * 1024));
```

- Per-route body limit, not one generous global limit. Small JSON by default; uploads with their own
  limit.
- Time limit per request and per external operation.
- Size limit during deserialization: do not trust `Content-Length`; cut the stream.
- Depth/nesting limit for recursive formats.
- Reject unexpected `Content-Type` and validate the schema with `serde` + `deny_unknown_fields` when
  the contract allows.

```rust
use std::time::Duration;
use tower::timeout::TimeoutLayer;

let app = app.layer(TimeoutLayer::new(Duration::from_secs(10)));
```

## Headers and TLS

```rust
use tower_http::set_header::SetResponseHeaderLayer;
use axum::http::{header, HeaderValue};

let app = app
    .layer(SetResponseHeaderLayer::if_not_present(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
    ));
```

- TLS in transit is mandatory; `rustls` or termination at the proxy with HSTS.
- `X-Frame-Options: DENY` or `frame-ancestors 'none'`; CSP without `unsafe-inline` when possible.
- Explicit CORS with allowed origins, never `*` with credentials.
- `Referrer-Policy`, `Permissions-Policy` according to the product.
- Do not trust `X-Forwarded-For` unless the proxy is authenticated and configured to overwrite it;
  otherwise rate limiting is trivially bypassed.
- `TraceLayer` for request ID and latency, with redaction of sensitive headers.

## CSRF

- With cookie authentication, every state mutation requires a CSRF token and `SameSite`.
- Double-submit pattern: `HttpOnly` cookie + token in the form/header, compared in constant time. Or
  use a maintained dedicated crate (e.g. `axum-csrf`) following its guide.
- Verify `Origin`/`Sec-Fetch-Site` as defense in depth, not as a replacement.
- API endpoints using `Authorization: Bearer` do not need CSRF if they do not accept cookies; do not
  mix both modes in the same endpoint.

## SSRF and outbound calls

```rust
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .connect_timeout(Duration::from_secs(2))
    .redirect(reqwest::redirect::Policy::none())
    .build()?;
```

- Allowlist of schemes (`https`), hosts, and ports.
- Resolve DNS and reject private, loopback, link-local, and cloud metadata IPs, including after
  redirects (better: no automatic redirects).
- Do not accept arbitrary user URLs without an allowlist; for webhooks, validate and limit.
- Timeout, maximum response size, and cancellation.
- Log destination and result, not credentials or tokens in the query string.

## Database

```rust
// Good: parameterized query, checked at compile time
let user = sqlx::query_as!(
    User,
    "SELECT id, email FROM users WHERE id = $1 AND tenant_id = $2",
    user_id,
    tenant_id
)
.fetch_optional(&pool)
.await?;
```

- Building SQL with `format!` or input concatenation is forbidden.
- Every query filters by `tenant_id` (or the equivalent trust boundary) on the server.
- Do not return the driver error to the client.
- Pool with a maximum size, acquisition timeout, and query timeout.
- Versioned, reviewed migrations; never a destructive `DROP`/`ALTER` without a plan.
- In `sqlx`, if you do not use macros, review the SQL manually: string queries are not validated.

## Authorization in handlers

- One extractor per concern: `AuthUser`, `Tenant`, `RequirePermission<P>`.
- The extractor resolves identity and tenant from the session, never from the body or path.
- The path only identifies the resource; authorization checks membership.
- Default deny: if there is no explicit rule for the role, deny.
- Avoid scattered `if user.role == "admin"`: centralize in a testable policy.

## Errors and panics in handlers

- `CatchPanicLayer` (tower-http) so a panic does not drop the connection without a controlled
  response.
- Even so, a panic is a bug: the `unwrap`/`panic` lints must prevent it in production.
- `IntoResponse` maps errors to stable codes without leaking details (see `error-handling.md`).
- 404 vs 403: do not reveal the existence of another tenant's resources; use 404 when appropriate.

## Rate limiting and abuse

- `tower_governor` or equivalent, keyed by IP + account + sensitive route.
- Different limits for login, registration, recovery, search, and writes.
- 429 responses with `Retry-After`; do not permanently lock out a victim.
- Work budget per request: number of queries, response size, fan-out to services.

## Observability

- Structured `tracing` with `request_id`, route, method, user (ID, not PII), latency, and outcome.
- Redact `Authorization`, `Cookie`, `Set-Cookie`, tokens, and bodies containing personal data.
- Do not log the full body by default; if debugging requires it, use sampling and redaction.
- Metrics for 4xx/5xx errors, p95/p99 latency, pool and queue saturation.
- Audit sensitive actions: who, what, when, on which resource, and the outcome.

## Service checklist

- [ ] Body limit, timeout, and deserialization size limit per route.
- [ ] TLS/HSTS, CSP, `X-Content-Type-Options`, frame protection, explicit CORS.
- [ ] CSRF when cookies are used; correct `SameSite`.
- [ ] SSRF: allowlist, no redirects, timeouts, private IPs blocked.
- [ ] Parameterized SQL and tenant filtering in every query.
- [ ] Mandatory auth/tenant/permission extractors on protected routes.
- [ ] Rate limiting on sensitive surfaces.
- [ ] Logs without secrets or unnecessary PII; auditing of sensitive actions.
- [ ] Negative tests: 401, 403, another tenant's 404, 413, 429, timeout, malformed input.
