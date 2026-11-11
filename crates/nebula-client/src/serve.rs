//! Finding the `nebula serve` that serves this daemon: the port its port
//! file (`nebula_core::paths::serve_port_path`) names, once a server
//! answers there as `nebula serve` does, naming itself in the `Server`
//! header of every response. A server killed before it could remove its
//! file leaves one naming a port nothing listens on, or another program
//! has taken since.

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
