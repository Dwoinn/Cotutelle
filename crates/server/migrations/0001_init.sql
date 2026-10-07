-- Schéma initial de Cotutelle. Les horodatages sont en secondes Unix (UTC),
-- les journées en texte AAAA-MM-JJ dans le fuseau du foyer.

CREATE TABLE parents (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE children (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    birth_year INTEGER,
    emoji      TEXT NOT NULL,
    color      TEXT NOT NULL,
    policy     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- kind = 'agent'   : appareil avec agent installé, comptes de session rattachés à des enfants.
-- kind = 'network' : appareil sans agent, filtré par le DNS du serveur selon son adresse IP.
CREATE TABLE devices (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('agent', 'network')),
    token_hash    TEXT UNIQUE,
    hostname      TEXT,
    os            TEXT,
    agent_version TEXT,
    ip            TEXT,
    -- Appareil réseau attribué à un enfant : applique sa politique.
    child_id      TEXT REFERENCES children (id) ON DELETE SET NULL,
    -- Appareil réseau partagé : politique propre (« profil appareil »).
    policy        TEXT,
    os_accounts   TEXT NOT NULL DEFAULT '[]',
    last_seen     INTEGER,
    created_at    INTEGER NOT NULL
);

CREATE TABLE device_accounts (
    device_id  TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    os_account TEXT NOT NULL,
    child_id   TEXT NOT NULL REFERENCES children (id) ON DELETE CASCADE,
    PRIMARY KEY (device_id, os_account)
);

-- Sessions web : un parent connecté, ou un enfant arrivé par le lien de son appareil.
CREATE TABLE web_sessions (
    token_hash TEXT PRIMARY KEY,
    parent_id  TEXT REFERENCES parents (id) ON DELETE CASCADE,
    child_id   TEXT REFERENCES children (id) ON DELETE CASCADE,
    device_id  TEXT REFERENCES devices (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

CREATE TABLE enroll_tokens (
    token_hash  TEXT PRIMARY KEY,
    device_name TEXT NOT NULL,
    expires_at  INTEGER NOT NULL
);

CREATE TABLE child_links (
    token_hash TEXT PRIMARY KEY,
    child_id   TEXT NOT NULL REFERENCES children (id) ON DELETE CASCADE,
    device_id  TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

-- Exceptions temporaires. Portent sur un enfant ou sur un appareil réseau partagé.
CREATE TABLE grants (
    id         TEXT PRIMARY KEY,
    child_id   TEXT REFERENCES children (id) ON DELETE CASCADE,
    device_id  TEXT REFERENCES devices (id) ON DELETE CASCADE,
    target     TEXT NOT NULL,
    starts_at  INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    granted_by TEXT REFERENCES parents (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    CHECK (child_id IS NOT NULL OR device_id IS NOT NULL)
);
CREATE INDEX grants_expiry ON grants (expires_at);

CREATE TABLE requests (
    id          TEXT PRIMARY KEY,
    child_id    TEXT NOT NULL REFERENCES children (id) ON DELETE CASCADE,
    device_id   TEXT REFERENCES devices (id) ON DELETE SET NULL,
    target      TEXT NOT NULL,
    minutes     INTEGER,
    message     TEXT,
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied')),
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER,
    resolved_by TEXT REFERENCES parents (id) ON DELETE SET NULL
);
CREATE INDEX requests_status ON requests (status, created_at);

-- Temps d'écran : valeur absolue par appareil, compte et journée.
CREATE TABLE usage (
    device_id  TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    os_account TEXT NOT NULL,
    day        TEXT NOT NULL,
    child_id   TEXT REFERENCES children (id) ON DELETE CASCADE,
    seconds    INTEGER NOT NULL,
    PRIMARY KEY (device_id, os_account, day)
);
CREATE INDEX usage_child_day ON usage (child_id, day);

-- Activité réseau agrégée par jour et par site (niveau 2).
-- child_id vaut '' pour un appareil réseau partagé.
CREATE TABLE dns_daily (
    day       TEXT NOT NULL,
    device_id TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    child_id  TEXT NOT NULL DEFAULT '',
    domain    TEXT NOT NULL,
    allowed   INTEGER NOT NULL DEFAULT 0,
    blocked   INTEGER NOT NULL DEFAULT 0,
    reason    TEXT,
    PRIMARY KEY (day, device_id, child_id, domain)
);
CREATE INDEX dns_daily_child ON dns_daily (child_id, day);

CREATE TABLE alerts (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    device_id  TEXT REFERENCES devices (id) ON DELETE CASCADE,
    child_id   TEXT REFERENCES children (id) ON DELETE CASCADE,
    message    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    acked_at   INTEGER
);
CREATE INDEX alerts_open ON alerts (acked_at, created_at);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
