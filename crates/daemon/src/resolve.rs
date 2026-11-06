//! Resolving a node's name past the guard.
//!
//! The guard drops every DNS query but the daemon's own. On Linux those carry
//! [`RESOLVER_MARK`], which only the daemon can set — `SO_MARK` needs
//! `CAP_NET_ADMIN`; on Windows the guard admits the daemon's program by its
//! application id. Either way this lets a node's name out and nothing else.
//!
//! The queries are plain DNS to public resolvers. A query reveals only the
//! node's name, and the connection that follows reveals its address anyway.
//! IPv4 only, as the tunnel is.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
#[cfg(unix)]
use wanderer_tunnel::guard::RESOLVER_MARK;

/// Asked in turn until one answers.
const RESOLVERS: [Ipv4Addr; 2] = [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)];
const TIMEOUT: Duration = Duration::from_secs(3);
/// A classic DNS message over UDP is at most 512 bytes; EDNS answers can be
/// larger, and an A query for one name never needs more than this.
const MAX_ANSWER: usize = 1232;

const TYPE_A: u16 = 1;
const CLASS_IN: u16 = 1;

/// The IPv4 addresses of `host`; empty when it has none.
pub(crate) async fn resolve(host: &str) -> std::io::Result<Vec<Ipv4Addr>> {
    let mut last = std::io::Error::other("no resolver answered");
    for resolver in RESOLVERS {
        match ask(resolver, host).await {
            Ok(addresses) => return Ok(addresses),
            Err(e) => last = e,
        }
    }
    Err(last)
}

async fn ask(resolver: Ipv4Addr, host: &str) -> std::io::Result<Vec<Ipv4Addr>> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    #[cfg(unix)]
    socket.set_mark(RESOLVER_MARK)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).into())?;
    let socket = UdpSocket::from_std(socket.into())?;
    socket.connect(SocketAddrV4::new(resolver, 53)).await?;

    // Unpredictable, with the kernel's random source port, so an off-path
    // spoofer has 32 bits to guess.
    let id: u16 = rand::random();
    socket.send(&query(id, host)).await?;
    let mut buf = vec![0; MAX_ANSWER];
    let n = tokio::time::timeout(TIMEOUT, socket.recv(&mut buf))
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
    answer(id, &buf[..n]).ok_or_else(|| std::io::Error::other("unreadable DNS answer"))
}

/// An A query for `host`, recursion desired.
fn query(id: u16, host: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(18 + host.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // standard query, recursion desired
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // one question
    for label in host.trim_end_matches('.').split('.') {
        // Labels are checked before they get here: at most 63 bytes.
        q.push(u8::try_from(label.len()).unwrap_or(63));
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&TYPE_A.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    q
}

/// The A records of an answer to query `id`, or `None` if it is not one.
fn answer(id: u16, msg: &[u8]) -> Option<Vec<Ipv4Addr>> {
    let u16_at = |i: usize| Some(u16::from_be_bytes([*msg.get(i)?, *msg.get(i + 1)?]));
    if u16_at(0)? != id || msg.get(2)? & 0x80 == 0 {
        return None; // not ours, or not an answer
    }
    if msg.get(3)? & 0x0f != 0 {
        return Some(Vec::new()); // NXDOMAIN and the like: no address
    }
    let questions = u16_at(4)?;
    let answers = u16_at(6)?;

    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(msg, at)? + 4;
    }
    let mut out = Vec::new();
    for _ in 0..answers {
        at = skip_name(msg, at)?;
        let (kind, class, len) = (u16_at(at)?, u16_at(at + 2)?, usize::from(u16_at(at + 8)?));
        let data = msg.get(at + 10..at + 10 + len)?;
        if kind == TYPE_A && class == CLASS_IN && len == 4 {
            out.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        at += 10 + len;
    }
    Some(out)
}

/// Where the name at `at` ends.
fn skip_name(msg: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = *msg.get(at)?;
        match len {
            0 => return Some(at + 1),
            l if l & 0xc0 == 0xc0 => return Some(at + 2), // a pointer ends the name
            l => at += 1 + usize::from(l),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_standard_query() {
        let q = query(0xabcd, "node.example.org");
        assert_eq!(&q[..4], &[0xab, 0xcd, 0x01, 0x00]);
        assert_eq!(&q[12..17], b"\x04node");
        assert_eq!(&q[q.len() - 5..], &[0, 0, 1, 0, 1]);
    }

    /// An answer as resolvers send it: the question, a CNAME, then an A
    /// record, names compressed.
    fn reply(id: u16, rcode: u8) -> Vec<u8> {
        let mut m = query(id, "a.example");
        m[2] = 0x81;
        m[3] = 0x80 | rcode;
        m[7] = 2; // two answers
        // CNAME a.example -> b, pointer to the question name
        m.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 4, 1, b'b', 0xc0, 14]);
        // A for the alias
        m.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 9]);
        m
    }

    #[test]
    fn reads_the_a_records_past_a_cname() {
        assert_eq!(
            answer(7, &reply(7, 0)),
            Some(vec![Ipv4Addr::new(203, 0, 113, 9)])
        );
    }

    #[test]
    fn refuses_answers_to_other_queries_and_reports_no_name() {
        assert_eq!(answer(8, &reply(7, 0)), None, "another id");
        assert_eq!(answer(7, &reply(7, 3)), Some(vec![]), "NXDOMAIN");
        assert_eq!(answer(7, &reply(7, 0)[..20]), None, "cut short");
    }
}
