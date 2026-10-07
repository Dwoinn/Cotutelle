<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt, resetCatalog } from '#lib/app.svelte.ts';
	import About from '#lib/components/About.svelte';
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

<div class="mx-auto max-w-2xl space-y-5">
	<h1>Réglages</h1>

	{#if settings}
		<section class="panel">
			<h2>Listes de sites</h2>
			{#if settings.blocklists.categories > 0}
				<p class="display mt-3 text-4xl">{fmt.format(settings.blocklists.entries)}</p>
				<p class="muted">
					sites connus, dans {settings.blocklists.categories} catégories. Mises à jour {ago(settings.blocklists.updated_at)}.
				</p>
			{:else}
				<p class="bg-sun-soft mt-3 rounded-2xl px-4 py-3">Pas encore téléchargées. Le filtrage par catégorie est inactif.</p>
			{/if}
			<button class="btn-quiet mt-4" disabled={refreshing} onclick={refreshLists}>
				{refreshing ? 'Téléchargement en cours…' : 'Mettre à jour maintenant'}
			</button>
			<p class="hint">Listes de l'Université Toulouse Capitole, licence CC BY-SA 4.0, rafraîchies chaque jour.</p>
		</section>

		<section class="panel">
			<h2>Notifications sur téléphone</h2>
			<p class="muted mt-1 mb-4 text-[0.95rem]">
				Recevez les demandes des enfants et les alertes avec l'application gratuite
				<a class="underline underline-offset-2" href="https://ntfy.sh" target="_blank" rel="noopener">ntfy</a>. Choisissez
				un nom de sujet difficile à deviner, abonnez-vous-y dans l'application, puis collez son adresse ici.
			</p>
			<form
				class="space-y-2"
				onsubmit={(e) => {
					e.preventDefault();
					save({ ntfy_url: settings!.ntfy_url }, 'Notifications enregistrées');
				}}
			>
				<label class="label" for="ntfy">Adresse du sujet</label>
				<input id="ntfy" class="field" bind:value={settings.ntfy_url} placeholder="https://ntfy.sh/famille-x7k2m9q4" inputmode="url" autocapitalize="off" />
				<p class="hint">Laissez vide pour désactiver. Le texte des notifications passe par le serveur ntfy indiqué.</p>
				<button class="btn-primary">Enregistrer</button>
			</form>
		</section>

		<section class="panel">
			<h2>Réseau et historique</h2>
			<form
				class="mt-4 space-y-4"
				onsubmit={(e) => {
					e.preventDefault();
					save({ upstream_dns: settings!.upstream_dns, retention_days: settings!.retention_days }, 'Réglages enregistrés');
				}}
			>
				<div>
					<label class="label" for="dns">Résolveurs DNS de secours</label>
					<input id="dns" class="field tabular-nums" bind:value={settings.upstream_dns} autocapitalize="off" />
					<p class="hint">Séparés par des virgules. Par défaut Quad9, qui écarte aussi les sites malveillants.</p>
				</div>
				<div>
					<label class="label" for="retention">Conserver l'activité pendant</label>
					<div class="flex items-center gap-3">
						<input id="retention" type="number" inputmode="numeric" min="1" max="365" class="field w-28" bind:value={settings.retention_days} />
						<span>jours</span>
					</div>
				</div>
				<button class="btn-primary">Enregistrer</button>
			</form>
		</section>
	{/if}

	<section class="panel">
		<h2>Parents</h2>
		<div class="rows mt-1">
			{#each parents as parent (parent.id)}
				<div class="row justify-between">
					<span class="font-semibold">
						{parent.name}
						{#if parent.id === app.parent?.id}<span class="tag-quiet ml-1">vous</span>{/if}
					</span>
					{#if parent.id !== app.parent?.id}
						{#if removing === parent.id}
							<span class="flex gap-2">
								<button class="btn-danger btn-sm" onclick={() => removeParent(parent)}>Supprimer</button>
								<button class="btn-quiet btn-sm" onclick={() => (removing = null)}>Annuler</button>
							</span>
						{:else}
							<button class="btn-ghost btn-sm text-muted" onclick={() => (removing = parent.id)}>Supprimer</button>
						{/if}
					{/if}
				</div>
			{/each}
		</div>
		<form class="border-line mt-2 space-y-3 border-t pt-4" onsubmit={addParent}>
			<h3>Ajouter un parent</h3>
			<div class="grid gap-3 sm:grid-cols-2">
				<div>
					<label class="label" for="parent-name">Nom</label>
					<input id="parent-name" class="field" bind:value={newParent.name} required />
				</div>
				<div>
					<label class="label" for="parent-password">Mot de passe</label>
					<input id="parent-password" type="password" class="field" bind:value={newParent.password} minlength="8" autocomplete="new-password" required />
				</div>
			</div>
			<button class="btn-quiet">Ajouter</button>
		</form>
	</section>

	<section class="panel">
		<h2>Mon mot de passe</h2>
		<form class="mt-4 space-y-3" onsubmit={changePassword}>
			<div class="grid gap-3 sm:grid-cols-2">
				<div>
					<label class="label" for="pw-current">Actuel</label>
					<input id="pw-current" type="password" class="field" bind:value={passwords.current} autocomplete="current-password" required />
				</div>
				<div>
					<label class="label" for="pw-new">Nouveau</label>
					<input id="pw-new" type="password" class="field" bind:value={passwords.new} minlength="8" autocomplete="new-password" required />
				</div>
			</div>
			<button class="btn-quiet">Modifier</button>
		</form>
	</section>

	<About version={settings?.version} />
</div>
