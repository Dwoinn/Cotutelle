<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt, resetCatalog } from '#lib/app.svelte.ts';
	import { ago } from '#lib/format.ts';
	import type { Settings } from '#lib/types.ts';
	import { onMount } from 'svelte';

	type ParentRow = { id: string; name: string; created_at: number };

	let settings = $state<Settings | null>(null);
	let parents = $state<ParentRow[]>([]);
	let newParent = $state({ name: '', password: '' });
	let passwords = $state({ current: '', new: '' });
	let refreshing = $state(false);
	let removing = $state<string | null>(null);

	const fmt = new Intl.NumberFormat('fr-FR');

	async function load() {
		settings = (await attempt(() => api.get<Settings>('/settings'))) ?? settings;
		parents = (await attempt(() => api.get<ParentRow[]>('/parents'))) ?? parents;
	}

	onMount(load);

	async function save(fields: Partial<Settings>, message: string) {
		if (await attempt(() => api.put('/settings', fields), message)) load();
	}

	async function refreshLists() {
		if (!settings) return;
		const before = settings.blocklists.updated_at;
		if (!(await attempt(() => api.post('/blocklists/refresh'), 'Téléchargement lancé'))) return;
		refreshing = true;
		// Le téléchargement prend une à deux minutes : on surveille son arrivée.
		for (let i = 0; i < 60 && refreshing; i++) {
			await new Promise((resolve) => setTimeout(resolve, 5000));
			const fresh = await api.get<Settings>('/settings').catch(() => null);
			if (fresh && fresh.blocklists.updated_at !== before) {
				settings = fresh;
				resetCatalog();
				break;
			}
		}
		refreshing = false;
	}

	async function addParent(event: SubmitEvent) {
		event.preventDefault();
		if (await attempt(() => api.post('/parents', newParent), `${newParent.name} peut maintenant se connecter`)) {
			newParent = { name: '', password: '' };
			load();
		}
	}

	async function removeParent(parent: ParentRow) {
		if (await attempt(() => api.del(`/parents/${parent.id}`), `${parent.name} supprimé`)) {
			removing = null;
			load();
		}
	}

	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		if (await attempt(() => api.put('/parents/me/password', passwords), 'Mot de passe modifié')) {
			passwords = { current: '', new: '' };
		}
	}
</script>

<div class="space-y-6">
	<h1>Réglages</h1>

	{#if settings}
		<section class="card space-y-3">
			<h2>Listes de blocage</h2>
			{#if settings.blocklists.categories > 0}
				<p class="text-sm">
					<strong>{fmt.format(settings.blocklists.entries)}</strong> sites dans
					<strong>{settings.blocklists.categories}</strong> catégories, mis à jour {ago(settings.blocklists.updated_at)}.
				</p>
			{:else}
				<p class="pill-warn rounded-2xl px-4 py-3 text-sm">
					Les listes ne sont pas encore téléchargées. Le filtrage par catégorie est inactif.
				</p>
			{/if}
			<p class="muted text-xs">
				Source : listes de l'Université Toulouse Capitole (licence CC BY-SA 4.0), rafraîchies automatiquement chaque
				jour.
			</p>
			<button class="btn-soft" disabled={refreshing} onclick={refreshLists}>
				{refreshing ? 'Téléchargement en cours…' : 'Mettre à jour maintenant'}
			</button>
		</section>

		<section class="card space-y-3">
			<h2>Notifications sur téléphone</h2>
			<p class="muted">
				Recevez les demandes des enfants et les alertes avec l'application gratuite
				<a class="underline" href="https://ntfy.sh" target="_blank" rel="noopener">ntfy</a>. Choisissez un nom de
				sujet difficile à deviner, abonnez-vous à ce sujet dans l'application, puis collez son adresse ici.
			</p>
			<form
				class="flex flex-wrap gap-2"
				onsubmit={(e) => {
					e.preventDefault();
					save({ ntfy_url: settings!.ntfy_url }, 'Notifications enregistrées');
				}}
			>
				<input class="input flex-1" bind:value={settings.ntfy_url} placeholder="https://ntfy.sh/famille-dupont-x7k2m9" aria-label="Adresse du sujet ntfy" />
				<button class="btn-primary">Enregistrer</button>
			</form>
			<p class="muted text-xs">
				Laissez vide pour désactiver. Le texte des notifications transite par le serveur ntfy indiqué.
			</p>
		</section>

		<section class="card space-y-4">
			<h2>Réseau et confidentialité</h2>
			<form
				class="space-y-4"
				onsubmit={(e) => {
					e.preventDefault();
					save({ upstream_dns: settings!.upstream_dns, retention_days: settings!.retention_days }, 'Réglages enregistrés');
				}}
			>
				<div>
					<label class="label" for="dns">Résolveurs DNS utilisés pour les sites autorisés</label>
					<input id="dns" class="input font-mono" bind:value={settings.upstream_dns} />
					<p class="muted mt-1 text-xs">Séparés par des virgules. Par défaut Quad9, qui bloque aussi les sites malveillants.</p>
				</div>
				<div>
					<label class="label" for="retention">Conservation de l'historique d'activité</label>
					<div class="flex items-center gap-2">
						<input id="retention" type="number" min="1" max="365" class="input w-24" bind:value={settings.retention_days} />
						<span class="muted">jours</span>
					</div>
				</div>
				<button class="btn-primary">Enregistrer</button>
			</form>
		</section>
	{/if}

	<section class="card space-y-4">
		<h2>Parents</h2>
		<ul class="divide-y divide-slate-100 dark:divide-slate-800">
			{#each parents as parent (parent.id)}
				<li class="flex items-center justify-between gap-3 py-2">
					<span class="font-medium">
						{parent.name}
						{#if parent.id === app.parent?.id}<span class="muted text-xs font-normal">· vous</span>{/if}
					</span>
					{#if parent.id !== app.parent?.id}
						{#if removing === parent.id}
							<span class="flex gap-2">
								<button class="btn-danger btn-sm" onclick={() => removeParent(parent)}>Confirmer</button>
								<button class="btn-ghost btn-sm" onclick={() => (removing = null)}>Annuler</button>
							</span>
						{:else}
							<button class="btn-ghost btn-sm" onclick={() => (removing = parent.id)}>Supprimer</button>
						{/if}
					{/if}
				</li>
			{/each}
		</ul>
		<form class="grid gap-2 sm:grid-cols-[1fr_1fr_auto]" onsubmit={addParent}>
			<input class="input" bind:value={newParent.name} placeholder="Nom du second parent" aria-label="Nom" required />
			<input type="password" class="input" bind:value={newParent.password} placeholder="Mot de passe (8 caractères min.)" aria-label="Mot de passe" minlength="8" autocomplete="new-password" required />
			<button class="btn-soft">Ajouter</button>
		</form>
	</section>

	<section class="card space-y-3">
		<h2>Mon mot de passe</h2>
		<form class="grid gap-2 sm:grid-cols-[1fr_1fr_auto]" onsubmit={changePassword}>
			<input type="password" class="input" bind:value={passwords.current} placeholder="Mot de passe actuel" aria-label="Mot de passe actuel" autocomplete="current-password" required />
			<input type="password" class="input" bind:value={passwords.new} placeholder="Nouveau mot de passe" aria-label="Nouveau mot de passe" minlength="8" autocomplete="new-password" required />
			<button class="btn-soft">Modifier</button>
		</form>
	</section>

	{#if settings}
		<p class="muted text-center text-xs">Cotutelle {settings.version}</p>
	{/if}
</div>
