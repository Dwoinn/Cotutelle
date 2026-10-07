# Interface web Cotutelle

SvelteKit 3 (Svelte 5, mode runes) et Tailwind 4, compilée en application
monopage statique et servie par `cotutelle-server`.

```bash
pnpm install
COTUTELLE_API=http://127.0.0.1:8087 pnpm dev   # rechargement à chaud, API relayée
pnpm check                                      # typage
pnpm build                                      # sortie dans build/
```

| Route | Rôle |
|-------|------|
| `/` | Tableau de bord : enfants, demandes, alertes, actions rapides |
| `/enfants/[id]` | Activité, horaires et temps, filtres, profil |
| `/appareils` | Ordinateurs avec agent, appareils sans agent |
| `/reglages` | Listes, notifications, réseau, comptes parents |
| `/moi` | Espace enfant ; `?apercu=<id>` pour l'aperçu parent |
| `/connexion`, `/installation` | Accès parents |

Les imports internes passent par `#lib/…` avec l'extension du fichier.
Les types de `src/lib/types.ts` reflètent les structures de `cotutelle-common`.
