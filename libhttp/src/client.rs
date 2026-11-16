//! Request/response flow over an injectable [`Transport`].
//!
//! [`execute`] is the only entry point a tool needs: build the request head,
//! send it (and the body for POST), read the response head, decide the body
//! framing and stream the body into a [`BodySink`]. The connection is closed on
//! every path, success or error.
//!
//! Informational (1xx) responses are skipped, up to a bounded number, so a
//! `100 Continue` never reaches the caller as the final response.

use alloc::vec::Vec;

use crate::body::{body_framing, BodyReader, BodySink};
use crate::error::HttpError;
use crate::limits::Limits;
use crate::parse::{HeadScanner, ResponseHead};
use crate::transport::{Close, Deadline, Read, Transport, Write};
use crate::{BodySource, Request};

/// Maximum number of 1xx responses tolerated before the next final response.
pub const MAX_INFORMATIONAL_RESPONSES: usize = 5;

/// Perform one HTTP/1.1 request.
///
/// `deadline` bounds the whole exchange; `limits` bound the head and body and
/// are also used by the transport for its per-operation inactivity timeouts.
pub fn execute<T: Transport>(
    transport: &mut T,
    request: &Request<'_>,
    limits: &Limits,
    deadline: Deadline,
    sink: &mut dyn BodySink,
) -> Result<ResponseHead, HttpError> {
    limits.validate()?;

    let mut conn = transport.connect(&request.url.host, request.url.port, deadline)?;

    let prepared = write_request(&mut conn, request, deadline)
        .and_then(|()| read_head(&mut conn, limits, deadline));
    let (head, leftover) = match prepared {
        Ok(value) => value,
        Err(err) => {
            conn.close();
            return Err(err);
        }
    };

    let framing = match body_framing(request.method, &head) {
        Ok(framing) => framing,
        Err(err) => {
            conn.close();
            return Err(err);
        }
    };

    sink.start(&head);

    let mut reader = BodyReader::with_buffered(conn, &leftover, deadline);
    let result = reader.read_body(framing, limits, sink);
    reader.into_inner().close();
    result.map(|_| head)
}

fn write_request<C: Write>(
    conn: &mut C,
    request: &Request<'_>,
    deadline: Deadline,
) -> Result<(), HttpError> {
    let head = request.build_head()?;
    conn.write_all(&head, deadline)?;
    if let Some(BodySource::Bytes(body)) = request.body {
        if !body.is_empty() {
            conn.write_all(body, deadline)?;
        }
    }
    Ok(())
}

fn read_head<C: Read>(
    conn: &mut C,
    limits: &Limits,
    deadline: Deadline,
) -> Result<(ResponseHead, Vec<u8>), HttpError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut scanner = HeadScanner::new(limits);
    let mut informational = 0usize;

    loop {
        let mut tmp = [0u8; 1024];
        let n = conn.read(&mut tmp, deadline)?;
        if n == 0 {
            return Err(HttpError::IncompleteBody);
        }
        buf.extend_from_slice(&tmp[..n]);

        loop {
            match scanner.scan(&buf)? {
                None => break,
                Some((head, consumed)) => {
                    if head.status / 100 == 1 {
                        informational += 1;
                        if informational > MAX_INFORMATIONAL_RESPONSES {
                            return Err(HttpError::TooManyHeaders);
                        }
                        // Drop the informational response and look for the
                        // real one in the bytes already buffered.
                        buf.drain(..consumed);
                        scanner.reset();
                        continue;
                    }
                    let leftover = buf.split_off(consumed);
                    return Ok((head, leftover));
                }
            }
        }
    }
}
