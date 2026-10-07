//! Petits outils : horloge, journées locales, identifiants, jetons.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn now() -> i64 {
    Utc::now().timestamp()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn from_ts(ts: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(ts, 0).single().unwrap_or_default()
}

pub fn day_str(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Journée en cours dans le fuseau du serveur.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Lundi de la semaine en cours.
pub fn week_start() -> NaiveDate {
    let today = today();
    today - Duration::days(i64::from(today.weekday().num_days_from_monday()))
}

/// Dernier instant de la journée locale, en secondes Unix.
pub fn end_of_today() -> i64 {
    let midnight = (today() + Duration::days(1)).and_hms_opt(0, 0, 0).expect("minuit existe");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map_or_else(|| now() + 86_400, |d| d.timestamp())
}

/// Jeton aléatoire de 256 bits, en hexadécimal.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Code court à recopier à la main lors de l'enrôlement : `K7QF-2MXD`.
pub fn new_enroll_code() -> String {
    // Sans 0/O ni 1/I pour éviter les confusions de lecture.
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let code: String =
        bytes.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    format!("{}-{}", &code[..4], &code[4..])
}

/// Forme canonique d'un code saisi : majuscules, sans espaces ni tiret.
pub fn normalize_code(code: &str) -> String {
    code.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase()
}

/// Les jetons ne sont jamais stockés en clair : seule leur empreinte l'est.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
