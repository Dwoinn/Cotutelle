//! Tests de bout en bout du relais contre un faux résolveur amont.

use cotutelle_dns::{Decision, DnsServer, Handler, MAX_TTL, message, query_a};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;

struct TestHandler {
    upstream: SocketAddr,
}

impl Handler for TestHandler {
    fn decide(&self, _client: IpAddr, name: &str) -> Decision {
        match name {
            "blocked.test" => Decision::Block,
            "canary.test" => Decision::NxDomain,
            "www.youtube.test" => Decision::Rewrite("restrict.youtube.test".into()),
            _ => Decision::Forward,
        }
    }

    fn upstreams(&self) -> Vec<SocketAddr> {
        vec![self.upstream]
    }
}

/// Faux amont : répond 203.0.113.7 avec un TTL d'une heure à toute question.
async fn fake_upstream() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            let reply =
                message::answer_a(&buf[..len], Ipv4Addr::new(203, 0, 113, 7), 3600).unwrap();
            socket.send_to(&reply, peer).await.unwrap();
        }
    });
    addr
}

async fn relay() -> SocketAddr {
    let upstream = fake_upstream().await;
    let server =
        DnsServer::bind("127.0.0.1:0".parse().unwrap(), Arc::new(TestHandler { upstream }))
            .await
            .unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.run());
    addr
}

#[tokio::test]
async fn forwards_and_clamps_ttl() {
    let reply = query_a(relay().await, "fr.wikipedia.test").await.unwrap();
    assert_eq!(reply.addresses, [IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))]);
    assert_eq!(reply.max_ttl, MAX_TTL);
}

#[tokio::test]
async fn blocks_with_null_address() {
    let reply = query_a(relay().await, "blocked.test").await.unwrap();
    assert_eq!(reply.addresses, [IpAddr::V4(Ipv4Addr::UNSPECIFIED)]);
    assert!(!reply.nxdomain);
}

#[tokio::test]
async fn canary_gets_nxdomain() {
    let reply = query_a(relay().await, "canary.test").await.unwrap();
    assert!(reply.nxdomain);
    assert!(reply.addresses.is_empty());
}

#[tokio::test]
async fn rewrite_answers_with_cname_and_target_address() {
    let reply = query_a(relay().await, "www.youtube.test").await.unwrap();
    assert_eq!(reply.cnames, ["restrict.youtube.test"]);
    assert_eq!(reply.addresses, [IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))]);
}

#[tokio::test]
async fn unreachable_upstream_yields_no_address() {
    // Port fermé : le relais doit répondre SERVFAIL plutôt que de rester muet.
    let dead: SocketAddr = "127.0.0.1:9".parse().unwrap();
    let server =
        DnsServer::bind("127.0.0.1:0".parse().unwrap(), Arc::new(TestHandler { upstream: dead }))
            .await
            .unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.run());
    let reply = query_a(addr, "fr.wikipedia.test").await.unwrap();
    assert!(reply.addresses.is_empty());
}
