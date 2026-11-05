//! Tests de bout en bout du relais contre un faux résolveur amont.

use cotutelle_dns::{Decision, DnsServer, Handler, MAX_TTL, message, query_a};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

struct TestHandler {
    upstream: SocketAddr,
}

/// Relais dont les amonts sont donnés tels quels, sans filtrage.
struct Chain(Vec<SocketAddr>);

impl Handler for Chain {
    fn decide(&self, _client: IpAddr, _name: &str) -> Decision {
        Decision::Forward
    }

    fn upstreams(&self) -> Vec<SocketAddr> {
        self.0.clone()
    }
}

/// Faux amont qui ne connaît aucun nom.
async fn nxdomain_upstream() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            socket.send_to(&message::nxdomain(&buf[..len]).unwrap(), peer).await.unwrap();
        }
    });
    addr
}

/// Faux amont qui répond NXDOMAIN après un délai.
async fn slow_nxdomain_upstream() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            tokio::time::sleep(Duration::from_millis(600)).await;
            socket.send_to(&message::nxdomain(&buf[..len]).unwrap(), peer).await.unwrap();
        }
    });
    addr
}

/// Faux amont en panne : SERVFAIL à tout.
async fn servfail_upstream() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            socket.send_to(&message::servfail(&buf[..len]).unwrap(), peer).await.unwrap();
        }
    });
    addr
}

async fn chain(upstreams: Vec<SocketAddr>) -> SocketAddr {
    let server =
        DnsServer::bind("127.0.0.1:0".parse().unwrap(), Arc::new(Chain(upstreams))).await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.run());
    addr
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

#[tokio::test]
async fn a_name_unknown_upstream_is_looked_up_on_the_next_one() {
    // Cas d'un nom privé : le premier amont l'ignore, le suivant le connaît.
    let relay = chain(vec![nxdomain_upstream().await, fake_upstream().await]).await;
    let reply = query_a(relay, "serveur.prive.test").await.unwrap();
    assert_eq!(reply.addresses, [IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))]);
}

/// Faux amont qui ne répond jamais : passerelle qui ne relaie pas le DNS.
async fn silent_upstream() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            socket.recv_from(&mut buf).await.unwrap();
        }
    });
    addr
}

#[tokio::test]
async fn a_silent_upstream_costs_a_short_delay_only() {
    let relay = chain(vec![silent_upstream().await, fake_upstream().await]).await;
    let started = std::time::Instant::now();
    let reply = query_a(relay, "fr.wikipedia.test").await.unwrap();
    assert_eq!(reply.addresses, [IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))]);
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
}

#[tokio::test]
async fn the_most_trusted_negative_answer_wins_over_a_later_one() {
    // Le premier amont répond NXDOMAIN tard, le second SERVFAIL tout de suite :
    // c'est le NXDOMAIN qui est rendu.
    let relay = chain(vec![slow_nxdomain_upstream().await, servfail_upstream().await]).await;
    let reply = query_a(relay, "inexistant.test").await.unwrap();
    assert!(reply.nxdomain);
}

#[tokio::test]
async fn a_name_unknown_everywhere_stays_nxdomain() {
    let relay = chain(vec![nxdomain_upstream().await, nxdomain_upstream().await]).await;
    let reply = query_a(relay, "inexistant.test").await.unwrap();
    assert!(reply.nxdomain && reply.addresses.is_empty());
}
