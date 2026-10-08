//! Catalogue fourni avec Cotutelle : catégories de listes mises en avant dans
//! l'interface et services nommés de départ (« YouTube », « TikTok »…).
//!
//! Les catégories viennent des listes UT1 de l'Université Toulouse Capitole
//! (licence CC BY-SA 4.0). Toute catégorie présente dans l'archive reste
//! utilisable même si elle n'est pas décrite ici.

use crate::Service;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CategoryInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Regroupement dans l'interface.
    pub group: &'static str,
    /// Cochée par défaut dans la politique proposée pour un enfant.
    pub recommended: bool,
}

const fn cat(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    group: &'static str,
    recommended: bool,
) -> CategoryInfo {
    CategoryInfo { id, label, description, group, recommended }
}

pub const CATEGORIES: &[CategoryInfo] = &[
    cat("adult", "Contenus pour adultes", "Pornographie et sites érotiques", "protection", true),
    cat(
        "mixed_adult",
        "Contenus choquants",
        "Sites mêlant contenus adultes et tout public",
        "protection",
        true,
    ),
    cat(
        "agressif",
        "Violence et haine",
        "Sites violents, racistes ou extrémistes",
        "protection",
        true,
    ),
    cat(
        "dangerous_material",
        "Produits dangereux",
        "Explosifs, armes, substances",
        "protection",
        true,
    ),
    cat("drogue", "Drogues", "Promotion ou vente de drogues", "protection", true),
    cat("sect", "Sectes", "Mouvements sectaires", "protection", true),
    cat("dating", "Rencontres", "Sites et applications de rencontre", "protection", true),
    cat("gambling", "Jeux d'argent", "Casinos et paris en ligne", "protection", true),
    cat(
        "arjel",
        "Paris en ligne agréés",
        "Opérateurs de jeux agréés en France",
        "protection",
        true,
    ),
    cat("lingerie", "Lingerie", "Boutiques de lingerie", "protection", false),
    cat("malware", "Logiciels malveillants", "Sites diffusant des virus", "securite", true),
    cat("phishing", "Hameçonnage", "Faux sites volant des identifiants", "securite", true),
    cat(
        "cryptojacking",
        "Minage caché",
        "Sites minant de la cryptomonnaie à l'insu du visiteur",
        "securite",
        true,
    ),
    cat("stalkerware", "Logiciels espions", "Outils de surveillance clandestine", "securite", true),
    cat("hacking", "Piratage informatique", "Outils et tutoriels d'intrusion", "securite", true),
    cat("warez", "Téléchargement illégal", "Logiciels et films piratés", "securite", true),
    cat("ddos", "Attaques en ligne", "Services d'attaque par déni de service", "securite", true),
    cat(
        "doh",
        "DNS chiffré tiers",
        "Résolveurs DNS-over-HTTPS permettant de contourner le filtrage",
        "contournement",
        true,
    ),
    cat(
        "vpn",
        "VPN",
        "Services de VPN permettant de contourner le filtrage",
        "contournement",
        true,
    ),
    cat(
        "redirector",
        "Proxys web",
        "Sites relais permettant de contourner le filtrage",
        "contournement",
        true,
    ),
    cat(
        "strict_redirector",
        "Proxys web (strict)",
        "Liste étendue de sites relais",
        "contournement",
        true,
    ),
    cat("residential-proxies", "Proxys résidentiels", "Réseaux de proxys", "contournement", true),
    cat(
        "remote-control",
        "Prise en main à distance",
        "Outils de contrôle à distance",
        "contournement",
        false,
    ),
    cat("social_networks", "Réseaux sociaux", "Facebook, Instagram, TikTok, X…", "loisirs", false),
    cat("games", "Jeux en ligne", "Sites de jeux vidéo", "loisirs", false),
    cat("audio-video", "Vidéo et musique", "Plateformes de streaming", "loisirs", false),
    cat("chat", "Messageries", "Tchats et messageries en ligne", "loisirs", false),
    cat("forums", "Forums", "Forums de discussion", "loisirs", false),
    cat("blog", "Blogs", "Plateformes de blogs", "loisirs", false),
    cat("manga", "Mangas", "Sites de lecture de mangas", "loisirs", false),
    cat("celebrity", "People", "Actualité des célébrités", "loisirs", false),
    cat("shopping", "Achats en ligne", "Boutiques en ligne", "loisirs", false),
    cat("ai", "Intelligence artificielle", "Assistants et générateurs IA", "loisirs", false),
    cat("publicite", "Publicité", "Régies publicitaires et traqueurs", "confort", false),
    cat(
        "shortener",
        "Raccourcisseurs de liens",
        "Liens courts masquant la destination",
        "confort",
        false,
    ),
    cat("fakenews", "Désinformation", "Sites de fausses informations", "confort", false),
    cat("tricheur", "Triche scolaire", "Sites de devoirs tout faits", "confort", false),
    cat("bitcoin", "Cryptomonnaies", "Échange et minage de cryptomonnaies", "confort", false),
    cat("filehosting", "Hébergement de fichiers", "Sites de partage de fichiers", "confort", false),
    cat("webmail", "Messagerie web", "Boîtes mail en ligne", "confort", false),
];

/// Libellés des groupes, dans l'ordre d'affichage.
pub const CATEGORY_GROUPS: &[(&str, &str)] = &[
    ("protection", "Protection de l'enfant"),
    ("securite", "Sécurité"),
    ("contournement", "Contournement du filtrage"),
    ("loisirs", "Loisirs et réseaux"),
    ("confort", "Confort"),
];

/// Service fourni avec Cotutelle.
///
/// Ce n'est qu'un point de départ : les parents peuvent en changer le nom et
/// les sites, ou ajouter leurs propres services. Le catalogue d'une famille
/// est une donnée du serveur, distribuée aux agents avec l'état de l'appareil
/// (voir [`crate::Service`]).
#[derive(Debug, Clone, Copy)]
pub struct BuiltinService {
    pub id: &'static str,
    pub label: &'static str,
    /// Tous les domaines dont le service a besoin, sous-domaines inclus : le
    /// site lui-même, mais aussi ses vidéos, ses images, son API. Le premier
    /// est le site principal, celui dont l'interface reprend le logo.
    pub domains: &'static [&'static str],
}

const fn svc(
    id: &'static str,
    label: &'static str,
    domains: &'static [&'static str],
) -> BuiltinService {
    BuiltinService { id, label, domains }
}

pub const SERVICES: &[BuiltinService] = &[
    svc(
        "youtube",
        "YouTube",
        &[
            "youtube.com",
            "youtu.be",
            "youtube-nocookie.com",
            "googlevideo.com",
            "ytimg.com",
            "youtubei.googleapis.com",
            "youtube.googleapis.com",
            "youtubekids.com",
            "yt3.ggpht.com",
            "yt3.googleusercontent.com",
        ],
    ),
    svc("twitch", "Twitch", &["twitch.tv", "ttvnw.net", "jtvnw.net", "twitchcdn.net"]),
    svc(
        "tiktok",
        "TikTok",
        &[
            "tiktok.com",
            "tiktokv.com",
            "tiktokcdn.com",
            "tiktokcdn-eu.com",
            "musical.ly",
            "byteoversea.com",
            "ibytedtos.com",
        ],
    ),
    svc("instagram", "Instagram", &["instagram.com", "cdninstagram.com", "ig.me"]),
    svc("snapchat", "Snapchat", &["snapchat.com", "sc-cdn.net", "snap-dev.net", "snapkit.co"]),
    svc(
        "discord",
        "Discord",
        &["discord.com", "discord.gg", "discordapp.com", "discordapp.net", "discord.media"],
    ),
    svc("whatsapp", "WhatsApp", &["whatsapp.com", "whatsapp.net", "wa.me"]),
    svc("roblox", "Roblox", &["roblox.com", "rbxcdn.com", "rbx.com", "robloxlabs.com"]),
    svc(
        "fortnite",
        "Fortnite / Epic Games",
        &["fortnite.com", "epicgames.com", "epicgames.dev", "unrealengine.com"],
    ),
    svc(
        "minecraft",
        "Minecraft en ligne",
        &["minecraft.net", "minecraftservices.com", "mojang.com"],
    ),
    svc(
        "netflix",
        "Netflix",
        &["netflix.com", "nflxvideo.net", "nflximg.net", "nflxext.com", "nflxso.net"],
    ),
    svc(
        "disneyplus",
        "Disney+",
        &["disneyplus.com", "disney-plus.net", "dssott.com", "bamgrid.com"],
    ),
    svc(
        "chatgpt",
        "ChatGPT",
        &["chatgpt.com", "openai.com", "oaistatic.com", "oaiusercontent.com"],
    ),
];

/// Les services fournis, sous la forme échangée avec les agents.
pub fn builtin_services() -> Vec<Service> {
    SERVICES
        .iter()
        .map(|s| Service {
            id: s.id.to_string(),
            label: s.label.to_string(),
            domains: s.domains.iter().map(|d| d.to_string()).collect(),
        })
        .collect()
}

/// Domaines « canaris » : leur blocage en NXDOMAIN signale aux navigateurs
/// et à iCloud qu'ils ne doivent pas activer leur DNS chiffré.
pub const CANARY_DOMAINS: &[&str] =
    &["use-application-dns.net", "mask.icloud.com", "mask-h2.icloud.com"];

/// Noms réécrits vers `restrict.youtube.com` quand le mode restreint est imposé.
pub const YOUTUBE_RESTRICT_HOSTS: &[&str] = &[
    "www.youtube.com",
    "m.youtube.com",
    "youtube.com",
    "youtubei.googleapis.com",
    "youtube.googleapis.com",
    "www.youtube-nocookie.com",
];

pub const YOUTUBE_RESTRICT_TARGET: &str = "restrict.youtube.com";

/// Catégories recommandées pour la politique par défaut d'un enfant.
pub fn recommended_categories() -> impl Iterator<Item = &'static str> {
    CATEGORIES.iter().filter(|c| c.recommended).map(|c| c.id)
}
