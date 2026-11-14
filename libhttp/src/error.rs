//! HTTP client errors.
//!
//! Every failure the client can report has its own variant, so tools can map
//! them to exit codes and messages without parsing strings.

/// Failure of an HTTP request or of parsing its response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    /// The URL is not an absolute `http://` / `https://` URL we can use.
    InvalidUrl,
    /// The request is inconsistent (e.g. a body on GET/HEAD).
    InvalidRequest,
    /// Name resolution failed.
    Dns,
    /// The TCP connection could not be established.
    Connect,
    /// A deadline expired.
    Timeout,
    /// The TLS handshake or the certificate check failed.
    Tls,
    /// The status line is not `HTTP/1.x <code> [reason]`.
    MalformedStatus,
    /// A header line is malformed, or a header is forbidden.
    MalformedHeader,
    /// A chunked body is malformed (size line, data CRLF or trailer).
    MalformedChunk,
    /// The response head exceeded the byte or header-count limit.
    TooManyHeaders,
    /// The response uses a transfer coding we do not implement.
    UnsupportedTransfer,
    /// The response is compressed with an encoding we cannot decode.
    UnsupportedEncoding,
    /// The body exceeded `Limits::max_body_bytes`.
    BodyTooLarge,
    /// The connection closed before the declared body was complete.
    IncompleteBody,
    /// The socket failed.
    Io,
    /// The caller cancelled the request.
    Cancelled,
}

impl HttpError {
    /// Short, stable, lowercase description (suitable for CLI output).
    pub fn as_str(self) -> &'static str {
        match self {
            HttpError::InvalidUrl => "invalid URL",
            HttpError::InvalidRequest => "invalid request",
            HttpError::Dns => "name resolution failed",
            HttpError::Connect => "connection failed",
            HttpError::Timeout => "timed out",
            HttpError::Tls => "TLS failure",
            HttpError::MalformedStatus => "malformed status line",
            HttpError::MalformedHeader => "malformed header",
            HttpError::MalformedChunk => "malformed chunked body",
            HttpError::TooManyHeaders => "response head too large",
            HttpError::UnsupportedTransfer => "unsupported transfer coding",
            HttpError::UnsupportedEncoding => "unsupported content encoding",
            HttpError::BodyTooLarge => "body too large",
            HttpError::IncompleteBody => "incomplete body",
            HttpError::Io => "I/O error",
            HttpError::Cancelled => "cancelled",
        }
    }
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
