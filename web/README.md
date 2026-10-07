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
| `/` | Accueil : demandes à traiter, un cadran et un bouclier par enfant |
| `/enfants/[id]` | Activité, temps et horaires, filtre, profil |
| `/appareils` | Ordinateurs avec agent, appareils sans agent |
| `/reglages` | Listes, notifications, réseau, comptes parents |
| `/moi` | Espace enfant ; `?apercu=<id>` pour l'aperçu parent |
| `/connexion`, `/installation` | Accès parents |

## Identité

La protection encadre, le temps vit à l'intérieur. Deux formes, deux tons :

- **Le bouclier**, en bleu nuit, pour la protection et le filtrage.
- **Le cadran**, en jaune soleil, pour le temps.

Le logo réunit les deux : deux moitiés de bouclier, comme deux bras, autour
d'un cadran. Dans l'interface, chaque enfant a ces deux instruments côte à
côte (`Dial.svelte`, `Shield.svelte`) ; chacun est une zone tactile qui ouvre
ses actions dans une feuille de bas d'écran (`TimeSheet`, `FilterSheet`).

| Nom | Valeur | Emploi |
|-----|--------|--------|
| Nuit | `#16203a` | Encre, bouclier, action principale ; fond du thème sombre |
| Aube | `#eef2f6` | Fond de page clair |
| Soleil | `#ffc53d` | Le temps, ce qui attend une réponse |
| Braise | `#d9541c` | Bientôt fini, alerte |
| Menthe | `#12805f` | Accordé, connecté |
| Brume | `#5a6780` | Texte secondaire |

Les couleurs passent par des variables CSS (`src/app.css`). Le thème sombre
et l'espace enfant « de nuit » (`data-phase`) les redéfinissent : aucun
composant n'utilise de variante `dark:`.

Typographie : Bricolage Grotesque pour les titres et tous les chiffres,
Atkinson Hyperlegible Next pour le texte. Les deux sont embarquées, aucun
appel à un serveur de polices.

## Règles d'interface

- Cibles tactiles d'au moins 48 px ; une action principale par écran.
- Les actions s'ouvrent dans une feuille (`Sheet.svelte`), en bas d'écran sur
  téléphone, centrée sur grand écran.
- Pas d'émoji : icônes Lucide au trait, monogrammes pour les enfants.
- Rien ne dépend du survol, ni d'une animation pour être visible.
- Durées avec espaces insécables (`format.ts`), jamais de « 30 / min ».

Les imports internes passent par `#lib/…` avec l'extension du fichier.
Les types de `src/lib/types.ts` reflètent les structures de `cotutelle-common`.
Les icônes PNG de `static/` se régénèrent avec `scripts/build-icons.py`.
