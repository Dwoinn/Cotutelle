<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt, resetCatalog } from '#lib/app.svelte.ts';
	import type { Service } from '#lib/types.ts';
	import ServiceLogo from './ServiceLogo.svelte';
	import Sheet from './Sheet.svelte';

	// Un service : son nom et tous les sites dont il a besoin. Sert à en
	// ajouter un, à compléter un service existant, à le retirer.
	let {
		open = $bindable(false),
		service,
		onsaved,
		onremoved
	}: {
		open: boolean;
		/** Le service à modifier ; `null` pour en ajouter un. */
		service: Service | null;
		onsaved: (service: Service, added: boolean) => void;
		onremoved: (id: string) => void;
	} = $props();

	let label = $state('');
	let sites = $state('');
	let busy = $state(false);
	let confirming = $state(false);

	// La feuille repart du service tel qu'il est enregistré à chaque ouverture.
	$effect(() => {
		if (!open) return;
		label = service?.label ?? '';
		sites = service?.domains.join('\n') ?? '';
		confirming = false;
	});

	const domains = $derived(
		sites
			.split(/[\n,]/)
			.map((line) => line.trim())
			.filter(Boolean)
	);
	const origin = $derived(
		!service ? '' : !service.builtin ? 'Ajouté par vous.' : service.modified ? 'Fourni avec Cotutelle, modifié par vous.' : 'Fourni avec Cotutelle.'
	);

	async function run<T>(action: () => Promise<T>, message: string): Promise<T | undefined> {
		busy = true;
		const result = await attempt(action, message);
		busy = false;
		if (result !== undefined) {
			resetCatalog();
			open = false;
		}
		return result;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body = { label, domains };
		const saved = service
			? await run(() => api.put<Service>(`/services/${service.id}`, body), 'Enregistré')
			: await run(() => api.post<Service>('/services', body), `${label.trim()} ajouté`);
		if (saved) onsaved(saved, !service);
	}

	/** Rend à un service fourni son nom et ses sites d'origine. */
	async function restore(current: Service) {
		if (!(await run(() => api.del(`/services/${current.id}`), `${current.label} rétabli`))) return;
		const fresh = await attempt(() => api.get<{ services: Service[] }>('/catalog'));
		const restored = fresh?.services.find((s) => s.id === current.id);
		if (restored) onsaved(restored, false);
	}

	async function remove(current: Service) {
		if (await run(() => api.del(`/services/${current.id}`), `${current.label} supprimé`)) onremoved(current.id);
	}
</script>

<Sheet bind:open title={service ? service.label : 'Ajouter un service'}>
	<form class="space-y-5" onsubmit={save}>
		{#if service}
			<div class="-mt-1 flex items-center gap-3">
				<ServiceLogo label={service.label} logo={service.logo} size={52} />
				<p class="muted text-sm">{origin}</p>
			</div>
		{/if}

		<div>
			<label class="label" for="service-name">Nom</label>
			<input id="service-name" class="field" bind:value={label} required maxlength="40" placeholder="Brawl Stars" autocomplete="off" />
		</div>

		<div>
			<label class="label" for="service-sites">Ses sites</label>
			<textarea
				id="service-sites"
				class="field resize-none leading-relaxed"
				rows={Math.min(14, Math.max(4, domains.length + 1))}
				bind:value={sites}
				required
				placeholder={'brawlstars.com\nsupercell.com'}
				autocapitalize="off"
				autocomplete="off"
				spellcheck="false"
			></textarea>
			<p class="hint">Un site par ligne, sous-domaines compris. Le premier donne le logo.</p>
		</div>

		<p class="bg-bg rounded-2xl px-4 py-3 text-sm leading-relaxed">
			Une appli charge rarement tout depuis un seul site : YouTube va chercher ses vidéos sur googlevideo.com. Si un
			service ouvert reste incomplet, ce qui manque apparaît dans l'activité de l'enfant, sous « Ce que le filtre a
			bloqué ».
		</p>

		<button class="btn-primary btn-block" disabled={busy}>{service ? 'Enregistrer' : 'Ajouter le service'}</button>

		{#if service?.builtin && service.modified}
			<button type="button" class="btn-quiet btn-block" disabled={busy} onclick={() => restore(service)}>
				Rétablir le nom et les sites d'origine
			</button>
		{:else if service && !service.builtin}
			{#if confirming}
				<div class="bg-cherry-soft rounded-2xl p-4">
					<p class="text-sm">
						{service.label} ne sera plus bloqué sur aucun appareil, et ses ouvertures en cours seront retirées.
					</p>
					<div class="mt-3 grid grid-cols-2 gap-2">
						<button type="button" class="btn bg-cherry text-surface" disabled={busy} onclick={() => remove(service)}>Supprimer</button>
						<button type="button" class="btn bg-surface" onclick={() => (confirming = false)}>Annuler</button>
					</div>
				</div>
			{:else}
				<button type="button" class="btn-ghost btn-block text-cherry" onclick={() => (confirming = true)}>Supprimer ce service</button>
			{/if}
		{/if}
	</form>
</Sheet>
