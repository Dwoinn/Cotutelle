//! Notifications aux parents par ntfy, dans les deux sens.
//!
//! Le serveur génère lui-même ses sujets, invisibles pour les parents : l'un
//! porte les notifications, que leurs téléphones suivent ; l'autre porte
//! leurs réponses, que le serveur suit par une connexion sortante
//! (`watch_replies`). Rien n'entre chez lui : un serveur réservé au réseau
//! local reçoit quand même la réponse d'un parent hors de la maison.
//!
//! Les boutons « Accorder » et « Refuser » d'une demande portent un jeton
//! signé par le serveur, lié à la demande et à la décision, valable un jour.
//! Une demande ne se tranche qu'une fois : rejouer un jeton ne fait rien.
//!
//! Chaque alerte est aussi enregistrée et visible dans l'interface.

use crate::state::Shared;
use crate::util;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::time::Duration;

pub const DEFAULT_SERVER: &str = "https://ntfy.sh";
/// Durée de validité d'un bouton de réponse.
const REPLY_TTL: i64 = 86_400;
/// Au-delà, une réponse tardive à une demande déjà tranchée ne vaut plus
/// de retour sur le téléphone : c'est un rejeu après reconnexion.
const FRESH_REPLY: i64 = 120;
const TOPIC_PREFIX: &str = "cotutelle-";

pub async fn setting(state: &Shared, key: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|(v,)| v)
        .filter(|v| !v.trim().is_empty())
}

pub async fn save_setting(state: &Shared, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Sujets et secret d'un serveur ntfy.
#[derive(Clone, Debug)]
pub struct Ntfy {
    pub server: String,
    pub topic: String,
    pub reply_topic: String,
    secret: String,
}

impl Ntfy {
    /// Adresse du sujet, à suivre dans l'application ntfy.
    pub fn subscribe_url(&self) -> String {
        format!("{}/{}", self.server, self.topic)
    }

    /// Lien `ntfy://` : l'application Android s'y abonne d'un geste.
    pub fn app_link(&self) -> String {
        let (scheme, host) = self.server.split_once("://").unwrap_or(("https", &self.server));
        let insecure = if scheme == "http" { "?secure=false" } else { "" };
        format!("ntfy://{host}/{}{insecure}", self.topic)
    }

    fn reply_url(&self) -> String {
        format!("{}/{}", self.server, self.reply_topic)
    }

    /// Jeton d'un bouton de réponse.
    pub fn reply(&self, request_id: &str, decision: Decision) -> Reply {
        let expires_at = util::now() + REPLY_TTL;
        let signature = self.sign(request_id, decision, expires_at);
        Reply { request_id: request_id.to_string(), decision, expires_at, signature }
    }

    /// Vérifie qu'un jeton vient bien d'ici et n'a pas expiré.
    pub fn verify(&self, reply: &Reply, now: i64) -> bool {
        let Ok(signature) = hex::decode(&reply.signature) else { return false };
        reply.expires_at > now
            && self
                .mac(&reply.request_id, reply.decision, reply.expires_at)
                .verify_slice(&signature)
                .is_ok()
    }

    fn sign(&self, request_id: &str, decision: Decision, expires_at: i64) -> String {
        hex::encode(self.mac(request_id, decision, expires_at).finalize().into_bytes())
    }

    fn mac(&self, request_id: &str, decision: Decision, expires_at: i64) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
            .expect("HMAC accepte toute taille de clé");
        mac.update(format!("{request_id}\n{}\n{expires_at}", decision.as_str()).as_bytes());
        mac
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Deny => "deny",
        }
    }
}

/// Ce qu'un bouton envoie au serveur, par le sujet des réponses.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    #[serde(rename = "request")]
    pub request_id: String,
    pub decision: Decision,
    #[serde(rename = "expires")]
    pub expires_at: i64,
    pub signature: String,
}

/// Réglages enregistrés, activés ou non. `None` tant que rien n'a été généré.
pub async fn stored(state: &Shared) -> Option<Ntfy> {
    let topic = setting(state, "ntfy_topic").await?;
    let reply_topic = setting(state, "ntfy_reply_topic").await?;
    let secret = setting(state, "ntfy_secret").await?;
    let server = setting(state, "ntfy_server").await.unwrap_or_else(|| DEFAULT_SERVER.to_string());
    Some(Ntfy { server, topic, reply_topic, secret })
}

pub async fn enabled(state: &Shared) -> bool {
    setting(state, "ntfy_enabled").await.is_some_and(|v| v == "1")
}

/// Réglages en service : activés et complets.
pub async fn active(state: &Shared) -> Option<Ntfy> {
    if enabled(state).await { stored(state).await } else { None }
}

fn random_topic() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let tail: String =
        (0..20).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect();
    format!("{TOPIC_PREFIX}{tail}")
}

/// Tire de nouveaux sujets et un nouveau secret : les anciens abonnements
/// ne reçoivent plus rien.
pub async fn reset(state: &Shared) -> Result<Ntfy> {
    save_setting(state, "ntfy_topic", &random_topic()).await?;
    save_setting(state, "ntfy_reply_topic", &random_topic()).await?;
    save_setting(state, "ntfy_secret", &util::new_token()).await?;
    state.ntfy_changed.notify_one();
    stored(state).await.context("réglages ntfy")
}

/// Active les notifications, en générant les sujets à la première fois.
pub async fn enable(state: &Shared) -> Result<Ntfy> {
    let ntfy = match stored(state).await {
        Some(ntfy) => ntfy,
        None => reset(state).await?,
    };
    save_setting(state, "ntfy_enabled", "1").await?;
    state.ntfy_changed.notify_one();
    Ok(ntfy)
}

/// Coupe les notifications sans oublier les sujets : les téléphones déjà
/// abonnés reprennent à la réactivation.
pub async fn disable(state: &Shared) -> Result<()> {
    save_setting(state, "ntfy_enabled", "0").await?;
    state.ntfy_changed.notify_one();
    Ok(())
}

/// Adresse d'un serveur ntfy, nettoyée. Vide : ntfy.sh.
pub fn clean_server(server: &str) -> Result<String, String> {
    let server = server.trim().trim_end_matches('/');
    if server.is_empty() {
        return Ok(DEFAULT_SERVER.to_string());
    }
    let Some((scheme, host)) = server.split_once("://") else {
        return Err("l'adresse du serveur ntfy doit commencer par https://".to_string());
    };
    if !matches!(scheme, "http" | "https") || host.is_empty() || host.contains('/') {
        return Err("adresse du serveur ntfy invalide, par exemple https://ntfy.sh".to_string());
    }
    Ok(server.to_string())
}

pub async fn set_server(state: &Shared, server: &str) -> Result<()> {
    save_setting(state, "ntfy_server", server).await?;
    state.ntfy_changed.notify_one();
    Ok(())
}

/// Avant la version 0.2.2, les parents collaient l'adresse complète d'un
/// sujet choisi par eux : on la reprend telle quelle pour ne pas les
/// désabonner, et on lui ajoute le sujet des réponses.
pub async fn migrate(state: &Shared) -> Result<()> {
    let Some(url) = setting(state, "ntfy_url").await else { return Ok(()) };
    if stored(state).await.is_none()
        && let Some((server, topic)) = url.trim_end_matches('/').rsplit_once('/')
        && let Ok(server) = clean_server(server)
        && !topic.is_empty()
    {
        save_setting(state, "ntfy_server", &server).await?;
        save_setting(state, "ntfy_topic", topic).await?;
        save_setting(state, "ntfy_reply_topic", &random_topic()).await?;
        save_setting(state, "ntfy_secret", &util::new_token()).await?;
        save_setting(state, "ntfy_enabled", "1").await?;
        tracing::info!(topic, "sujet ntfy repris de l'ancien réglage");
    }
    sqlx::query("DELETE FROM settings WHERE key = 'ntfy_url'").execute(&state.db).await?;
    Ok(())
}

/// QR du lien d'abonnement, en SVG.
pub fn qr_svg(ntfy: &Ntfy) -> Result<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(ntfy.app_link())?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(svg::Color("#16203a"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

async fn publish(
    state: &Shared,
    ntfy: &Ntfy,
    title: &str,
    body: &str,
    actions: Vec<Value>,
) -> Result<()> {
    // Le format JSON accepte l'UTF-8 là où les en-têtes HTTP sont en ASCII.
    let payload = json!({
        "topic": ntfy.topic,
        "title": title,
        "message": body,
        "tags": ["family"],
        "actions": actions,
    });
    state
        .http
        .post(format!("{}/", ntfy.server))
        .json(&payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Envoie une notification, sans jamais faire échouer l'appelant.
pub async fn push(state: &Shared, title: &str, body: &str) {
    let Some(ntfy) = active(state).await else { return };
    if let Err(e) = publish(state, &ntfy, title, body, Vec::new()).await {
        tracing::warn!(error = %e, "notification ntfy non envoyée");
    }
}

/// Envoie une notification de test ; l'erreur revient aux réglages.
pub async fn test(state: &Shared) -> Result<()> {
    let ntfy = active(state).await.context("notifications désactivées")?;
    publish(
        state,
        &ntfy,
        "Cotutelle : c'est en place",
        "Ce téléphone recevra les demandes des enfants et les alertes.",
        Vec::new(),
    )
    .await
    .context("le serveur ntfy n'a pas accepté la notification")
}

fn action(label: &str, url: &str, reply: &Reply) -> Value {
    json!({
        "action": "http",
        "label": label,
        "url": url,
        "method": "POST",
        "body": serde_json::to_string(reply).expect("jeton sérialisable"),
        "clear": true,
    })
}

/// Notifie une demande d'enfant, avec un bouton pour l'accorder telle
/// quelle et un pour la refuser.
pub async fn push_request(state: &Shared, request_id: &str, text: &str, approve_label: &str) {
    let Some(ntfy) = active(state).await else { return };
    let url = ntfy.reply_url();
    let actions = vec![
        action(approve_label, &url, &ntfy.reply(request_id, Decision::Approve)),
        action("Refuser", &url, &ntfy.reply(request_id, Decision::Deny)),
    ];
    if let Err(e) = publish(state, &ntfy, "Cotutelle : nouvelle demande", text, actions).await {
        tracing::warn!(error = %e, "notification ntfy non envoyée");
    }
}

pub async fn alert(
    state: &Shared,
    kind: &str,
    device_id: Option<&str>,
    child_id: Option<&str>,
    message: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO alerts (id, kind, device_id, child_id, message, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(util::new_id())
    .bind(kind)
    .bind(device_id)
    .bind(child_id)
    .bind(message)
    .bind(util::now())
    .execute(&state.db)
    .await?;
    push(state, "Cotutelle : alerte", message).await;
    Ok(())
}

/// Un événement du flux JSON de ntfy.
#[derive(Deserialize)]
struct Event {
    id: String,
    event: String,
    #[serde(default)]
    time: i64,
    #[serde(default)]
    message: String,
}

/// Suit le sujet des réponses tant que les notifications sont activées, et
/// se reconnecte en cas de coupure. Les réglages qui changent relancent
/// l'abonnement.
pub async fn watch_replies(state: Shared) {
    // Au premier abonnement, tout ce que ntfy garde (douze heures par
    // défaut) : une réponse arrivée pendant un arrêt du serveur n'est pas
    // perdue, et une demande déjà tranchée ignore son jeton.
    let mut since = String::from("all");
    let mut delay = 5;
    loop {
        let Some(ntfy) = active(&state).await else {
            state.ntfy_changed.notified().await;
            since = "all".to_string();
            continue;
        };
        tokio::select! {
            outcome = follow(&state, &ntfy, &mut since) => match outcome {
                Ok(()) => delay = 5,
                Err(e) => {
                    tracing::warn!(error = %e, delay, "réponses ntfy : reconnexion");
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    delay = (delay * 2).min(300);
                }
            },
            () = state.ntfy_changed.notified() => {
                since = "all".to_string();
                delay = 5;
            }
        }
    }
}

async fn follow(state: &Shared, ntfy: &Ntfy, since: &mut String) -> Result<()> {
    let url = format!("{}/json?since={since}", ntfy.reply_url());
    let response = state.http.get(&url).send().await?.error_for_status()?;
    tracing::info!(topic = %ntfy.reply_topic, "réponses ntfy suivies");
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        // ntfy envoie un signe de vie toutes les 45 secondes.
        let chunk = tokio::time::timeout(Duration::from_secs(120), stream.next())
            .await
            .context("plus de signe de vie du serveur ntfy")?;
        let Some(chunk) = chunk else { return Ok(()) };
        buffer.extend_from_slice(&chunk?);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let Ok(event) = serde_json::from_str::<Event>(line.trim()) else { continue };
            if event.event != "message" {
                continue;
            }
            *since = event.id.clone();
            handle_reply(state, ntfy, &event).await;
        }
    }
}

async fn handle_reply(state: &Shared, ntfy: &Ntfy, event: &Event) {
    let Ok(reply) = serde_json::from_str::<Reply>(&event.message) else {
        tracing::debug!("message ignoré sur le sujet des réponses");
        return;
    };
    let now = util::now();
    if !ntfy.verify(&reply, now) {
        tracing::warn!(request = %reply.request_id, "réponse ntfy à la signature invalide ou expirée");
        return;
    }
    let fresh = event.time > now - FRESH_REPLY;
    match crate::api::grants::answer_from_phone(state, &reply).await {
        Ok(Some(feedback)) => push(state, "Cotutelle", &feedback).await,
        Ok(None) if fresh => push(state, "Cotutelle", "Cette demande a déjà été traitée.").await,
        Ok(None) => {}
        Err(e) => {
            tracing::error!(error = %e, request = %reply.request_id, "réponse ntfy non appliquée")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ntfy() -> Ntfy {
        Ntfy {
            server: DEFAULT_SERVER.to_string(),
            topic: "cotutelle-abc".to_string(),
            reply_topic: "cotutelle-def".to_string(),
            secret: "secret".to_string(),
        }
    }

    #[test]
    fn reply_round_trip() {
        let n = ntfy();
        let now = util::now();
        let reply = n.reply("req-1", Decision::Approve);
        assert!(n.verify(&reply, now));
        // Rejoué après expiration, ou pour une autre décision : refusé.
        assert!(!n.verify(&reply, now + REPLY_TTL + 1));
        let mut forged = reply.clone();
        forged.decision = Decision::Deny;
        assert!(!n.verify(&forged, now));
        let mut other = reply.clone();
        other.request_id = "req-2".to_string();
        assert!(!n.verify(&other, now));
        // Signé avec un autre secret : refusé.
        let stranger = Ntfy { secret: "autre".to_string(), ..ntfy() };
        assert!(!stranger.verify(&reply, now));
        assert!(!n.verify(&Reply { signature: "zz".to_string(), ..reply.clone() }, now));
    }

    #[test]
    fn reply_serializes_compactly() {
        let reply = ntfy().reply("req-1", Decision::Deny);
        let json = serde_json::to_string(&reply).unwrap();
        assert!(json.contains("\"decision\":\"deny\""));
        let back: Reply = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request_id, "req-1");
        assert_eq!(back.decision, Decision::Deny);
    }

    #[test]
    fn links() {
        let n = ntfy();
        assert_eq!(n.subscribe_url(), "https://ntfy.sh/cotutelle-abc");
        assert_eq!(n.app_link(), "ntfy://ntfy.sh/cotutelle-abc");
        let local = Ntfy { server: "http://ntfy.maison:2586".to_string(), ..ntfy() };
        assert_eq!(local.app_link(), "ntfy://ntfy.maison:2586/cotutelle-abc?secure=false");
        assert_eq!(n.reply_url(), "https://ntfy.sh/cotutelle-def");
    }

    #[test]
    fn topics_are_random_and_valid() {
        let a = random_topic();
        let b = random_topic();
        assert_ne!(a, b);
        assert_eq!(a.len(), TOPIC_PREFIX.len() + 20);
        assert!(a.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn server_addresses() {
        assert_eq!(clean_server("").unwrap(), DEFAULT_SERVER);
        assert_eq!(clean_server(" https://ntfy.sh/ ").unwrap(), "https://ntfy.sh");
        assert_eq!(clean_server("http://ntfy.maison:2586").unwrap(), "http://ntfy.maison:2586");
        assert!(clean_server("ntfy.sh").is_err());
        assert!(clean_server("https://ntfy.sh/sujet").is_err());
        assert!(clean_server("ftp://ntfy.sh").is_err());
    }

    #[test]
    fn qr_is_svg() {
        let svg = qr_svg(&ntfy()).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("<svg"));
    }
}
