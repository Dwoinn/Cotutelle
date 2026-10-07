# Interface web Cotutelle

SvelteKit + Tailwind, servie en statique par `cotutelle-server`.

Non initialisée en phase 0. Pour démarrer (phase 1) :

```bash
cd web
pnpm dlx sv create . --template minimal --types ts
pnpm dlx sv add tailwindcss
pnpm install && pnpm dev
```

Deux espaces : `/` parents (authentifié), `/moi` enfant (depuis l'appareil,
sans mot de passe), plus la page de blocage `/bloque`.
