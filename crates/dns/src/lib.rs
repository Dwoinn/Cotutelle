//! Relais DNS filtrant.
//!
//! Chaque requête est soumise à un [`Handler`] qui rend une [`Decision`] :
//! relayer vers un résolveur amont, bloquer, ou réécrire vers un autre nom.
//! Le relais ne met rien en cache et plafonne les TTL renvoyés, pour qu'une
//! exception temporaire prenne effet (et expire) en moins d'une minute.

pub mod message;

use anyhow::{Context, Result, bail};
use cotutelle_common::Verdict;
use cotutelle_common::filter::BlockReason;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::timeout;

/// TTL maximal renvoyé aux clients, en secondes.
pub const MAX_TTL: u32 = 30;
/// TTL des réponses de blocage.
pub const BLOCK_TTL: u32 = 10;

const UDP_TIMEOUT: Duration = Duration::from_secs(2);
const TCP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_UDP_PACKET: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Relayer tel quel vers l'amont.
    Forward,
    /// Répondre `0.0.0.0` / `::` (ou une réponse vide pour les autres types).
    Block,
    /// Répondre NXDOMAIN.
    NxDomain,
    /// Répondre par un CNAME vers ce nom, résolu en amont.
    Rewrite(String),
}

impl From<Verdict> for Decision {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Allow => Decision::Forward,
            Verdict::Block(BlockReason::Canary) => Decision::NxDomain,
            Verdict::Block(_) => Decision::Block,
            Verdict::Rewrite(target) => Decision::Rewrite(target.to_string()),
        }
    }
}

/// Politique appliquée par le relais. Doit répondre vite : appelée pour
/// chaque requête, sur le chemin critique.
pub trait Handler: Send + Sync + 'static {
    /// `name` est le nom demandé, en minuscules et sans point final.
    fn decide(&self, client: IpAddr, name: &str) -> Decision;

    /// Résolveurs amont, essayés dans l'ordre.
    fn upstreams(&self) -> Vec<SocketAddr>;
}

/// Relais lié à une adresse, prêt à servir en UDP et en TCP.
pub struct DnsServer {
    udp: Arc<UdpSocket>,
    tcp: TcpListener,
    handler: Arc<dyn Handler>,
}

impl DnsServer {
    pub async fn bind(addr: SocketAddr, handler: Arc<dyn Handler>) -> Result<Self> {
        let udp = UdpSocket::bind(addr).await.with_context(|| format!("écoute UDP sur {addr}"))?;
        // En test on lie le port 0 : le TCP doit suivre le port choisi pour l'UDP.
        let tcp_addr = SocketAddr::new(addr.ip(), udp.local_addr()?.port());
        let tcp = TcpListener::bind(tcp_addr)
            .await
            .with_context(|| format!("écoute TCP sur {tcp_addr}"))?;
        Ok(Self { udp: Arc::new(udp), tcp, handler })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.udp.local_addr()?)
    }

    /// Sert jusqu'à l'arrêt du processus.
    pub async fn run(self) -> Result<()> {
        let Self { udp, tcp, handler } = self;
        let udp_loop = {
            let handler = handler.clone();
            async move {
                let mut buf = [0u8; MAX_UDP_PACKET];
                loop {
                    let (len, client) = match udp.recv_from(&mut buf).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::debug!(error = %e, "réception UDP");
                            continue;
                        }
                    };
                    let packet = buf[..len].to_vec();
                    let (udp, handler) = (udp.clone(), handler.clone());
                    tokio::spawn(async move {
                        if let Some(reply) =
                            answer(&packet, client.ip(), false, handler.as_ref()).await
                        {
                            let _ = udp.send_to(&reply, client).await;
                        }
                    });
                }
            }
        };
        let tcp_loop = async move {
            loop {
                let Ok((stream, client)) = tcp.accept().await else { continue };
                let handler = handler.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_tcp(stream, client.ip(), handler).await {
                        tracing::debug!(error = %e, "connexion TCP");
                    }
                });
            }
        };
        tokio::join!(udp_loop, tcp_loop);
        Ok(())
    }
}

async fn serve_tcp(mut stream: TcpStream, client: IpAddr, handler: Arc<dyn Handler>) -> Result<()> {
    loop {
        let Ok(Ok(packet)) = timeout(TCP_TIMEOUT, read_frame(&mut stream)).await else {
            return Ok(());
        };
        if let Some(reply) = answer(&packet, client, true, handler.as_ref()).await {
            write_frame(&mut stream, &reply).await?;
        }
    }
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let len = stream.read_u16().await? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_frame(stream: &mut TcpStream, packet: &[u8]) -> Result<()> {
    let len = u16::try_from(packet.len()).context("réponse DNS trop longue")?;
    stream.write_u16(len).await?;
    stream.write_all(packet).await?;
    stream.flush().await?;
    Ok(())
}

/// Traite une requête et rend la réponse à renvoyer, s'il y en a une.
async fn answer(
    packet: &[u8],
    client: IpAddr,
    tcp: bool,
    handler: &dyn Handler,
) -> Option<Vec<u8>> {
    let Some(question) = message::parse_question(packet) else {
        // Ni requête standard ni question unique : on relaie sans juger.
        return forward(packet, tcp, &handler.upstreams()).await.ok();
    };

    match handler.decide(client, &question.name) {
        Decision::Forward => match forward(packet, tcp, &handler.upstreams()).await {
            Ok(reply) => Some(message::clamp_ttl(&reply, MAX_TTL).unwrap_or(reply)),
            Err(e) => {
                tracing::debug!(name = %question.name, error = %e, "amont injoignable");
                message::servfail(packet)
            }
        },
        Decision::Block => message::blocked(packet, BLOCK_TTL),
        Decision::NxDomain => message::nxdomain(packet),
        Decision::Rewrite(target) => {
            let Some(upstream_query) = message::query_for(&target, &question) else {
                return message::servfail(packet);
            };
            match forward(&upstream_query, tcp, &handler.upstreams()).await {
                Ok(reply) => message::rewritten(packet, &target, &reply, MAX_TTL)
                    .or_else(|| message::servfail(packet)),
                Err(_) => message::servfail(packet),
            }
        }
    }
}

async fn forward(packet: &[u8], tcp: bool, upstreams: &[SocketAddr]) -> Result<Vec<u8>> {
    let mut last_error = None;
    for upstream in upstreams {
        let attempt = if tcp {
            forward_tcp(packet, *upstream).await
        } else {
            forward_udp(packet, *upstream).await
        };
        match attempt {
            Ok(reply) => return Ok(reply),
            Err(e) => last_error = Some(e),
        }
    }
    match last_error {
        Some(e) => Err(e),
        None => bail!("aucun résolveur amont configuré"),
    }
}

async fn forward_udp(packet: &[u8], upstream: SocketAddr) -> Result<Vec<u8>> {
    let local: SocketAddr = if upstream.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse()?;
    let socket = UdpSocket::bind(local).await?;
    socket.connect(upstream).await?;
    socket.send(packet).await?;
    let mut buf = [0u8; MAX_UDP_PACKET];
    let len = timeout(UDP_TIMEOUT, socket.recv(&mut buf)).await.context("délai dépassé")??;
    // Écarte une réponse qui ne correspond pas à notre requête.
    if len < 2 || packet.len() < 2 || buf[..2] != packet[..2] {
        bail!("identifiant de réponse inattendu");
    }
    Ok(buf[..len].to_vec())
}

async fn forward_tcp(packet: &[u8], upstream: SocketAddr) -> Result<Vec<u8>> {
    timeout(TCP_TIMEOUT, async {
        let mut stream = TcpStream::connect(upstream).await?;
        write_frame(&mut stream, packet).await?;
        read_frame(&mut stream).await
    })
    .await
    .context("délai dépassé")?
}

/// Interroge un relais DNS en UDP : sert aux tests et aux diagnostics.
pub async fn query_a(server: SocketAddr, name: &str) -> Result<message::Reply> {
    let packet = message::build_query(name, false).context("nom invalide")?;
    let reply = forward_udp(&packet, server).await?;
    message::parse_reply(&reply).context("réponse illisible")
}

pub use message::Reply;
