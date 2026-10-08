-- Catalogue des services de la famille. Les services fournis avec Cotutelle
-- ne sont pas recopiés ici : une ligne à leur identifiant remplace leur nom
-- et leurs sites, les autres lignes sont les services ajoutés par les parents.
CREATE TABLE services (
    id         TEXT PRIMARY KEY,
    label      TEXT NOT NULL,
    -- Tableau JSON de domaines, le site principal en premier.
    domains    TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- Logo d'un service, repris de son site principal. `image` est NULL quand la
-- recherche n'a rien donné : on ne la refait pas à chaque démarrage.
CREATE TABLE service_logos (
    service_id   TEXT PRIMARY KEY,
    domain       TEXT NOT NULL,
    content_type TEXT,
    image        BLOB,
    -- Icône d'application, pleine et opaque : affichée bord à bord.
    full_bleed   INTEGER NOT NULL DEFAULT 0,
    fetched_at   INTEGER NOT NULL
);

-- Jusqu'ici, tout service bloqué était proposé à l'enfant dans son espace :
-- les politiques existantes gardent ce comportement.
UPDATE children
SET policy = json_set(
    policy,
    '$.filter.requestable_services',
    json(json_extract(policy, '$.filter.blocked_services'))
)
WHERE json_valid(policy) AND json_type(policy, '$.filter.blocked_services') = 'array';
