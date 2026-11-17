//! Bounded HTTP/1.0 downloads through the real DNS/IPv4/TCP stack.
//! HTTPS, proxies, redirects and chunked transfer are not implemented here.
use super::{dns, ipif, tcp};
use alloc::{format, string::String, vec::Vec};
const LIMIT: usize = 4 * 1024 * 1024;
fn now() -> u64 {
    crate::hal::common::pit::get_system_time_ms()
}
fn poll() {
    crate::drivers::net::poll_all();
    tcp::check_retransmit_timers();
    core::hint::spin_loop();
}
struct Connection(u32);
impl Drop for Connection {
    fn drop(&mut self) {
        tcp::close(self.0);
    }
}
pub fn get(url: &str) -> Result<Vec<u8>, &'static str> {
    let target = url
        .strip_prefix("http://")
        .ok_or("Only http:// URLs are supported.")?;
    if target.len() > 2048 || target.bytes().any(|c| c <= 32 || c == 127) {
        return Err("Invalid URL.");
    }
    let (authority, path) = target
        .split_once('/')
        .map(|(a, p)| (a, format!("/{p}")))
        .unwrap_or((target, String::from("/")));
    if authority.contains('@') || authority.contains('#') {
        return Err("Invalid HTTP authority.");
    }
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        (h, p.parse::<u16>().map_err(|_| "Invalid TCP port.")?)
    } else {
        (authority, 80)
    };
    if host.is_empty() || port == 0 {
        return Err("Invalid HTTP host.");
    }
    let remote = dns::resolve(host).map_err(|_| "DNS lookup failed.")?;
    let local = ipif::get_outbound_ip().ok_or("No configured network interface.")?;
    let connection = Connection(
        tcp::connect(local, 0, remote, port).ok_or("Cannot allocate a TCP connection.")?,
    );
    let deadline = now().saturating_add(10000);
    while tcp::socket_state(connection.0) != tcp::TcpState::Established {
        poll();
        if now() >= deadline || tcp::socket_state(connection.0) == tcp::TcpState::Closed {
            return Err("TCP connection failed or timed out.");
        }
    }
    let request = format!("GET {path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    if tcp::send(connection.0, request.as_bytes()) != Some(request.len()) {
        return Err("TCP send failed.");
    }
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let mut body_start = None;
    let mut expected = None;
    loop {
        poll();
        if let Some(n) = tcp::receive(connection.0, &mut buffer) {
            if n > 0 {
                if bytes.len() + n > LIMIT + 16384 {
                    return Err("HTTP response exceeds the download limit.");
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
        }
        if body_start.is_none() {
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                if end > 16384 {
                    return Err("HTTP headers exceed the limit.");
                }
                let header =
                    core::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid HTTP headers.")?;
                let mut lines = header.split("\r\n");
                let status = lines.next().ok_or("Missing HTTP status.")?;
                if !status.starts_with("HTTP/1.") || status.split_whitespace().nth(1) != Some("200")
                {
                    return Err("The HTTP server did not return 200 OK.");
                }
                for line in lines {
                    let Some((key, value)) = line.split_once(':') else {
                        return Err("Malformed HTTP header.");
                    };
                    if key.eq_ignore_ascii_case("Content-Length") {
                        let length = value
                            .trim()
                            .parse::<usize>()
                            .map_err(|_| "Invalid content length.")?;
                        if length > LIMIT || expected.is_some_and(|n| n != length) {
                            return Err("Invalid or excessive content length.");
                        }
                        expected = Some(length);
                    }
                    if key.eq_ignore_ascii_case("Transfer-Encoding")
                        && !value.trim().eq_ignore_ascii_case("identity")
                    {
                        return Err("Chunked HTTP transfer is unsupported.");
                    }
                }
                body_start = Some(end + 4);
            } else if bytes.len() > 16384 {
                return Err("HTTP headers exceed the limit.");
            }
        }
        if let Some(start) = body_start {
            let received = bytes.len() - start;
            if received > LIMIT {
                return Err("HTTP body exceeds the download limit.");
            }
            if let Some(length) = expected {
                if received >= length {
                    return Ok(bytes[start..start + length].to_vec());
                }
            }
            if matches!(
                tcp::socket_state(connection.0),
                tcp::TcpState::CloseWait | tcp::TcpState::Closed
            ) && !tcp::has_buffered_data(connection.0)
            {
                if expected.is_some_and(|n| received != n) {
                    return Err("The HTTP response was truncated.");
                }
                return Ok(bytes[start..].to_vec());
            }
        }
        if now() >= deadline {
            return Err("HTTP download timed out.");
        }
    }
}
