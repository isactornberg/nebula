//! Finding the `nebula serve` that serves this daemon: the port its port
//! file (`nebula_core::paths::serve_port_path`) names, once a server
//! answers there as `nebula serve` does, naming itself in the `Server`
//! header of every response. A server killed before it could remove its
//! file leaves one naming a port nothing listens on, or another program
//! has taken since. And asking it, as a command does that reads what only
//! the server holds ([`call`]).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

/// The `Server` header every response of `nebula serve` carries.
pub const SERVER: &str = "nebula serve";

/// How long a server named in the port file has to accept the probe, and
/// then to answer it.
const PROBE_WAIT: Duration = Duration::from_millis(500);

/// The port `port_file` names, when `nebula serve` answers there.
pub fn serving_port(port_file: &Path) -> Option<u16> {
    let port = std::fs::read_to_string(port_file)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    answers(port).then_some(port)
}

/// Does `nebula serve` answer on `port`?
fn answers(port: u16) -> bool {
    let probe = || -> std::io::Result<String> {
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&at, PROBE_WAIT)?;
        stream.set_read_timeout(Some(PROBE_WAIT))?;
        stream.set_write_timeout(Some(PROBE_WAIT))?;
        let ask = format!("HEAD / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        stream.write_all(ask.as_bytes())?;
        let mut head = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut head)?;
        Ok(String::from_utf8_lossy(&head).into_owned())
    };
    probe().is_ok_and(|head| {
        head.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case("server") && value.trim() == SERVER
            })
        })
    })
}

/// What `nebula serve` on `port` answers `method` on `path`, with `headers`
/// and `body`, waiting up to `wait` for it: the status and the body. One
/// HTTP/1.0 request a connection, from this machine as its own page asks,
/// so the answer comes whole with no chunks.
pub fn call(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    wait: Duration,
) -> std::io::Result<(u16, String)> {
    let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&at, PROBE_WAIT)?;
    stream.set_read_timeout(Some(wait))?;
    stream.set_write_timeout(Some(wait))?;
    let mut ask = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        ask.push_str(&format!("{name}: {value}\r\n"));
    }
    ask.push_str("\r\n");
    stream.write_all(ask.as_bytes())?;
    stream.write_all(body)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    let answer = String::from_utf8_lossy(&answer);
    let unread = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not an HTTP answer");
    let (head, body) = answer.split_once("\r\n\r\n").ok_or_else(unread)?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(unread)?;
    Ok((status, body.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A listener on a free port that answers every connection with
    /// `head`, and the port.
    fn answering(head: &'static str) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut asked = Vec::new();
                let mut byte = [0];
                while !asked.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1)
                {
                    asked.push(byte[0]);
                }
                let _ = stream.write_all(head.as_bytes());
            }
        });
        port
    }

    #[test]
    fn only_a_server_that_names_itself_nebula_serve_is_one() {
        let ours = answering("HTTP/1.0 200 OK\r\nserver: nebula serve\r\n\r\n");
        assert!(answers(ours));
        let other = answering("HTTP/1.0 200 OK\r\nServer: nginx\r\n\r\n");
        assert!(!answers(other), "another program took the port");
        let mute = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        assert!(
            !answers(mute.local_addr().unwrap().port()),
            "accepts and says nothing"
        );
        let dead = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(!answers(dead), "nothing listens");
    }

    /// A call says who it is from as the page does, sends its headers and
    /// body, and reads the status and the body back.
    #[test]
    fn a_call_reads_the_status_and_body_of_the_answer() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let asked = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut byte = [0];
            while !asked.ends_with(b"{}") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                asked.push(byte[0]);
            }
            stream
                .write_all(
                    b"HTTP/1.0 409 Conflict\r\nServer: nebula serve\r\n\r\n{\"error\":\"no\"}",
                )
                .unwrap();
            String::from_utf8(asked).unwrap()
        });
        let got = call(
            port,
            "POST",
            "/api/x",
            &[("X-Nebula-Agent", "o")],
            b"{}",
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(got, (409, "{\"error\":\"no\"}".to_string()));
        let asked = asked.join().unwrap();
        assert!(asked.starts_with("POST /api/x HTTP/1.0\r\n"), "{asked}");
        for line in [
            format!("Host: 127.0.0.1:{port}\r\n"),
            format!("Origin: http://127.0.0.1:{port}\r\n"),
            "Content-Length: 2\r\n".into(),
            "X-Nebula-Agent: o\r\n".into(),
        ] {
            assert!(asked.contains(&line), "{line} in {asked}");
        }
        assert!(asked.ends_with("\r\n\r\n{}"), "{asked}");
    }

    #[test]
    fn the_port_file_names_the_port_to_probe() {
        let dir = std::env::temp_dir().join(format!("nebula-serve-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("serve.port");
        assert_eq!(serving_port(&file), None, "no file");
        let port = answering("HTTP/1.0 200 OK\r\nServer: nebula serve\r\n\r\n");
        std::fs::write(&file, format!("{port}\n")).unwrap();
        assert_eq!(serving_port(&file), Some(port));
        std::fs::write(&file, "not a port\n").unwrap();
        assert_eq!(serving_port(&file), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
