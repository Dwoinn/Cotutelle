//! Lecture et fabrication des messages DNS, isolées du reste du relais.

use cotutelle_common::normalize_domain;
use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, CNAME};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::BinEncodable;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(crate) struct Question {
    pub name: String,
    pub record_type: RecordType,
}

/// Extrait la question d'une requête standard à question unique.
pub(crate) fn parse_question(packet: &[u8]) -> Option<Question> {
    let message = Message::from_vec(packet).ok()?;
    if message.message_type() != MessageType::Query || message.op_code() != OpCode::Query {
        return None;
    }
    let [query] = message.queries() else { return None };
    Some(Question {
        name: normalize_domain(&query.name().to_ascii()),
        record_type: query.query_type(),
    })
}

/// Squelette de réponse reprenant l'identifiant et la question de la requête.
fn reply_to(packet: &[u8], code: ResponseCode) -> Option<Message> {
    let request = Message::from_vec(packet).ok()?;
    let mut reply = Message::new();
    reply
        .set_id(request.id())
        .set_message_type(MessageType::Response)
        .set_op_code(request.op_code())
        .set_recursion_desired(request.recursion_desired())
        .set_recursion_available(true)
        .set_response_code(code);
    reply.add_queries(request.queries().to_vec());
    Some(reply)
}

pub(crate) fn servfail(packet: &[u8]) -> Option<Vec<u8>> {
    reply_to(packet, ResponseCode::ServFail)?.to_bytes().ok()
}

pub(crate) fn nxdomain(packet: &[u8]) -> Option<Vec<u8>> {
    reply_to(packet, ResponseCode::NXDomain)?.to_bytes().ok()
}

/// Réponse de blocage : adresse nulle pour A/AAAA, réponse vide sinon.
pub(crate) fn blocked(packet: &[u8], ttl: u32) -> Option<Vec<u8>> {
    let mut reply = reply_to(packet, ResponseCode::NoError)?;
    let query = reply.queries().first()?.clone();
    let rdata = match query.query_type() {
        RecordType::A => Some(RData::A(A(Ipv4Addr::UNSPECIFIED))),
        RecordType::AAAA => Some(RData::AAAA(AAAA(Ipv6Addr::UNSPECIFIED))),
        _ => None,
    };
    if let Some(rdata) = rdata {
        reply.add_answer(Record::from_rdata(query.name().clone(), ttl, rdata));
    }
    reply.to_bytes().ok()
}

fn fqdn(name: &str) -> Option<Name> {
    let mut name = Name::from_ascii(name).ok()?;
    name.set_fqdn(true);
    Some(name)
}

/// Requête amont pour la cible d'une réécriture, du même type que l'originale.
pub(crate) fn query_for(target: &str, question: &Question) -> Option<Vec<u8>> {
    let mut message = Message::new();
    message
        .set_id(rand_id())
        .set_message_type(MessageType::Query)
        .set_op_code(OpCode::Query)
        .set_recursion_desired(true);
    message.add_query(Query::query(fqdn(target)?, question.record_type));
    message.to_bytes().ok()
}

/// Réponse CNAME `question → target`, complétée par les enregistrements amont.
pub(crate) fn rewritten(
    packet: &[u8],
    target: &str,
    upstream: &[u8],
    max_ttl: u32,
) -> Option<Vec<u8>> {
    let upstream = Message::from_vec(upstream).ok()?;
    let mut reply = reply_to(packet, upstream.response_code())?;
    let name = reply.queries().first()?.name().clone();
    reply.add_answer(Record::from_rdata(name, max_ttl, RData::CNAME(CNAME(fqdn(target)?))));
    for record in upstream.answers() {
        let mut record = record.clone();
        record.set_ttl(record.ttl().min(max_ttl));
        reply.add_answer(record);
    }
    reply.to_bytes().ok()
}

/// Plafonne les TTL d'une réponse. Rend `None` si rien n'est à changer ou si
/// le message n'est pas réencodable : l'appelant renvoie alors l'original.
pub(crate) fn clamp_ttl(packet: &[u8], max_ttl: u32) -> Option<Vec<u8>> {
    let mut message = Message::from_vec(packet).ok()?;
    // Les réponses signées ou tronquées sont transmises à l'identique.
    if message.truncated() || !message.signature().is_empty() {
        return None;
    }
    let mut changed = false;
    for record in message.answers_mut().iter_mut() {
        if record.ttl() > max_ttl {
            record.set_ttl(max_ttl);
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    message.to_bytes().ok()
}

fn rand_id() -> u16 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    RandomState::new().build_hasher().finish() as u16
}

pub(crate) fn build_query(name: &str, ipv6: bool) -> Option<Vec<u8>> {
    let record_type = if ipv6 { RecordType::AAAA } else { RecordType::A };
    query_for(name, &Question { name: name.to_string(), record_type })
}

/// Réponse DNS décodée, réduite à ce dont les tests et diagnostics ont besoin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub nxdomain: bool,
    pub addresses: Vec<IpAddr>,
    pub cnames: Vec<String>,
    pub max_ttl: u32,
}

pub(crate) fn parse_reply(packet: &[u8]) -> Option<Reply> {
    let message = Message::from_vec(packet).ok()?;
    let mut reply = Reply {
        nxdomain: message.response_code() == ResponseCode::NXDomain,
        addresses: Vec::new(),
        cnames: Vec::new(),
        max_ttl: 0,
    };
    for record in message.answers() {
        reply.max_ttl = reply.max_ttl.max(record.ttl());
        match record.data() {
            RData::A(a) => reply.addresses.push(IpAddr::V4(a.0)),
            RData::AAAA(a) => reply.addresses.push(IpAddr::V6(a.0)),
            RData::CNAME(c) => reply.cnames.push(normalize_domain(&c.0.to_ascii())),
            _ => {}
        }
    }
    Some(reply)
}

/// Fabrique une réponse A : utilisé par le faux résolveur amont des tests.
pub fn answer_a(packet: &[u8], address: Ipv4Addr, ttl: u32) -> Option<Vec<u8>> {
    let mut reply = reply_to(packet, ResponseCode::NoError)?;
    let name = reply.queries().first()?.name().clone();
    reply.add_answer(Record::from_rdata(name, ttl, RData::A(A(address))));
    reply.to_bytes().ok()
}
