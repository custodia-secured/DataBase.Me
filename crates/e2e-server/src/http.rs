// SPDX-License-Identifier: AGPL-3.0-or-later
//! Just enough HTTP/1.1 for this server, over the standard library, with a thread per connection.
//!
//! It replaces `tiny_http` 0.12, whose thread pool could leave a new connection queued behind a
//! kept-alive one. `TaskPool::spawn` counts a thread as idle until that thread has woken, so a burst
//! of connections could queue more work than there were threads free. Browsers keep connections
//! open, so the queued one waited, and a page's first load hung (docs/milestones/M3.md, Blockers).
//!
//! Request bodies need a `Content-Length`: browsers and Playwright send one, and chunks are refused.

use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::net::TcpStream;

/// The largest request body: base64 of a 32 MiB attachment, with room to spare.
const MAX_BODY: usize = 48 * 1024 * 1024;
/// The longest request line or header.
const MAX_LINE: u64 = 64 * 1024;
const MAX_HEADERS: usize = 100;

pub(crate) struct Request {
    pub(crate) method: String,
    /// The path and query, as sent.
    pub(crate) target: String,
    pub(crate) body: Vec<u8>,
    close: bool,
}

pub(crate) struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Reply {
    pub(crate) fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    pub(crate) fn with(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }
}

fn malformed(what: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what)
}

/// One line without its ending, or `None` if the connection closed first.
fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    reader
        .by_ref()
        .take(MAX_LINE)
        .read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.pop() != Some(b'\n') {
        return Err(malformed("a line is too long or cut short"));
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

/// The next request, or `None` if the client closed the connection between requests.
fn read_request(reader: &mut impl BufRead) -> io::Result<Option<Request>> {
    let Some(line) = read_line(reader)? else {
        return Ok(None);
    };
    let mut parts = line.split_ascii_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(malformed("bad request line"));
    };
    let mut close = version != "HTTP/1.1";
    let mut length = 0;
    for _ in 0..MAX_HEADERS {
        let line = read_line(reader)?.ok_or_else(|| malformed("the headers end early"))?;
        if line.is_empty() {
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            return Ok(Some(Request {
                method: method.to_owned(),
                target: target.to_owned(),
                body,
                close,
            }));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| malformed("bad header"))?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().map_err(|_| malformed("bad Content-Length"))?;
            if length > MAX_BODY {
                return Err(malformed("the body is too large"));
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(malformed("chunked bodies are not supported"));
        } else if name.eq_ignore_ascii_case("connection") {
            close = value.eq_ignore_ascii_case("close")
                || (close && !value.eq_ignore_ascii_case("keep-alive"));
        }
    }
    Err(malformed("too many headers"))
}

fn write_reply(
    stream: &mut impl Write,
    reply: &Reply,
    head_only: bool,
    close: bool,
) -> io::Result<()> {
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\n",
        reply.status,
        reply.body.len()
    );
    if close {
        head.push_str("Connection: close\r\n");
    }
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&reply.body)?;
    }
    stream.flush()
}

/// Answers a connection's requests until it closes. A read or write that fails means the client
/// went away, so there is no one left to tell.
pub(crate) fn serve(stream: TcpStream, handle: impl Fn(&Request) -> Reply) {
    // The head and the body are two writes, and neither should wait for the other's ACK.
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    loop {
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(error) => {
                if error.kind() == ErrorKind::InvalidData {
                    let refusal = Reply::new(400, error.to_string().into_bytes());
                    let _ = write_reply(&mut writer, &refusal, false, true);
                }
                return;
            }
        };
        let reply = handle(&request);
        let written = write_reply(&mut writer, &reply, request.method == "HEAD", request.close);
        if written.is_err() || request.close {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, TcpListener};
    use std::time::Duration;

    /// A server that answers with the request's method, target and body.
    fn echo_server() -> SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = stream.unwrap();
                std::thread::spawn(move || {
                    serve(stream, |request| {
                        let body = String::from_utf8_lossy(&request.body);
                        let text = format!("{} {} {body}", request.method, request.target);
                        Reply::new(200, text.into_bytes()).with("X-Test", "yes")
                    });
                });
            }
        });
        address
    }

    fn connect(address: SocketAddr) -> TcpStream {
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
    }

    /// Sends a request, and returns the status line, the headers and the body of the reply.
    fn ask(mut stream: &TcpStream, text: &str) -> (String, Vec<String>, String) {
        stream.write_all(text.as_bytes()).unwrap();
        let mut reader = BufReader::new(stream);
        let status = read_line(&mut reader).unwrap().unwrap();
        let mut headers = Vec::new();
        let mut length = 0;
        loop {
            let line = read_line(&mut reader).unwrap().unwrap();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().unwrap();
            }
            headers.push(line);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        (status, headers, String::from_utf8(body).unwrap())
    }

    #[test]
    fn a_connection_carries_one_request_after_another() {
        let stream = connect(echo_server());
        let (status, headers, body) = ask(
            &stream,
            "POST /ipc?a=1 HTTP/1.1\r\ncontent-LENGTH: 5\r\n\r\nhello",
        );
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert!(headers.contains(&"X-Test: yes".to_owned()));
        assert_eq!(body, "POST /ipc?a=1 hello");
        let (_, _, body) = ask(&stream, "GET /next HTTP/1.1\r\n\r\n");
        assert_eq!(body, "GET /next ");
    }

    /// The hang this module exists to end: many connections held open, as browsers hold them, and
    /// new ones still answered at once. `tiny_http`'s pool could leave one of these waiting.
    #[test]
    fn connections_held_open_leave_room_for_new_ones() {
        let address = echo_server();
        let mut held = Vec::new();
        for _ in 0..64 {
            let stream = connect(address);
            assert_eq!(ask(&stream, "GET / HTTP/1.1\r\n\r\n").2, "GET / ");
            held.push(stream);
        }
        let burst: Vec<_> = (0..64)
            .map(|_| {
                std::thread::spawn(move || {
                    let stream = connect(address);
                    ask(&stream, "GET /burst HTTP/1.1\r\n\r\n").2
                })
            })
            .collect();
        for thread in burst {
            assert_eq!(thread.join().unwrap(), "GET /burst ");
        }
        for stream in &held {
            assert_eq!(ask(stream, "GET /again HTTP/1.1\r\n\r\n").2, "GET /again ");
        }
    }

    #[test]
    fn a_head_request_gets_the_length_but_no_body() {
        let stream = connect(echo_server());
        (&stream)
            .write_all(b"HEAD /x HTTP/1.1\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        (&stream).read_to_string(&mut reply).unwrap();
        assert!(reply.contains("Content-Length: 8\r\n"), "{reply}");
        assert!(reply.ends_with("\r\n\r\n"), "{reply}");
    }

    #[test]
    fn a_client_that_asks_to_close_is_closed_on() {
        let stream = connect(echo_server());
        let (_, headers, _) = ask(&stream, "GET / HTTP/1.1\r\nConnection: close\r\n\r\n");
        assert!(headers.contains(&"Connection: close".to_owned()));
        assert_eq!((&stream).read(&mut [0; 1]).unwrap(), 0);
    }

    #[test]
    fn what_cannot_be_read_is_refused() {
        let address = echo_server();
        for request in [
            "nonsense\r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: many\r\n\r\n",
            "POST / HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n",
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "GET / HTTP/1.1\r\nno colon here\r\n\r\n",
        ] {
            let stream = connect(address);
            let (status, _, _) = ask(&stream, request);
            assert_eq!(status, "HTTP/1.1 400 Bad Request", "{request}");
            assert_eq!((&stream).read(&mut [0; 1]).unwrap(), 0, "{request}");
        }
    }
}
