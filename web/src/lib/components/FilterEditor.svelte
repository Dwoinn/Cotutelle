<script lang="ts">
	import type { Catalog, Policy } from '#lib/types.ts';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Switch from './Switch.svelte';

	// Réglage du filtre : services, catégories de sites, listes personnelles.
	let { filter = $bindable(), catalog }: { filter: Policy['filter']; catalog: Catalog } = $props();

	const fmt = new Intl.NumberFormat('fr-FR');

	function toggle(list: 'blocked_services' | 'blocked_categories', id: string) {
		filter[list] = filter[list].includes(id) ? filter[list].filter((x) => x !== id) : [...filter[list], id];
	}

	function setGroup(ids: string[], blocked: boolean) {
		const others = filter.blocked_categories.filter((c) => !ids.includes(c));
		filter.blocked_categories = blocked ? [...others, ...ids] : others;
	}

	const lines = (text: string) =>
		text
			.split(/[\n,]/)
			.map((l) => l.trim())
			.filter(Boolean);
</script>

{#snippet state(blocked: boolean)}
	<span class={blocked ? 'tag bg-ink text-surface' : 'tag-quiet'}>{blocked ? 'Bloqué' : 'Autorisé'}</span>
{/snippet}

<div class="space-y-8">
	{#if !catalog.blocklists_ready}
		<p class="bg-sun-soft rounded-2xl px-4 py-3 text-sm">
			Les listes de sites ne sont pas encore téléchargées. Les catégories s'appliqueront dès leur arrivée. Les
			services et vos listes personnelles fonctionnent déjà.
		</p>
	{/if}

	<section>
		<h2>Services</h2>
		<p class="muted mb-1 text-sm">Un service bloqué peut être ouvert pour un moment depuis l'accueil.</p>
		<div class="rows">
			{#each catalog.services as service (service.id)}
				{@const blocked = filter.blocked_services.includes(service.id)}
				<button type="button" class="row justify-between" aria-pressed={blocked} onclick={() => toggle('blocked_services', service.id)}>
					<span class="font-semibold">{service.label}</span>
					{@render state(blocked)}
				</button>
			{/each}
		</div>
		<div class="border-line mt-1 border-t">
			<Switch
				bind:checked={filter.youtube_restricted}
				label="Mode restreint de YouTube"
				description="Quand YouTube est ouvert, masque les vidéos signalées comme inadaptées."
			/>
		</div>
	</section>

	<section>
		<h2>Catégories de sites</h2>
		<p class="muted mb-3 text-sm">Listes de l'Université Toulouse Capitole, mises à jour chaque jour.</p>
		<div class="space-y-2">
			{#each catalog.groups as group (group.id)}
				{@const ids = group.categories.map((c) => c.id)}
				{@const count = ids.filter((id) => filter.blocked_categories.includes(id)).length}
				<details class="bg-bg group rounded-3xl">
					<summary class="flex min-h-14 list-none items-center justify-between gap-3 px-4 py-2 [&::-webkit-details-marker]:hidden">
						<span class="font-semibold">{group.label}</span>
						<span class="muted flex items-center gap-2 text-sm">
							{count} sur {ids.length} bloquées
							<ChevronDown size={18} class="transition-transform group-open:rotate-180" />
						</span>
					</summary>
					<div class="px-4 pb-3">
						<div class="mb-1 grid grid-cols-2 gap-2">
							<button class="btn-quiet btn-sm" onclick={() => setGroup(ids, true)}>Tout bloquer</button>
							<button class="btn-quiet btn-sm" onclick={() => setGroup(ids, false)}>Tout autoriser</button>
						</div>
						<div class="rows">
							{#each group.categories as category (category.id)}
								{@const blocked = filter.blocked_categories.includes(category.id)}
								<button type="button" class="row justify-between" aria-pressed={blocked} onclick={() => toggle('blocked_categories', category.id)}>
									<span class="min-w-0">
										<span class="block font-semibold">{category.label}</span>
										<span class="muted block text-sm">
											{category.description}{#if category.entries}, {fmt.format(category.entries)} sites{/if}
										</span>
									</span>
									{@render state(blocked)}
								</button>
							{/each}
						</div>
					</div>
				</details>
			{/each}
		</div>
	</section>

	<section class="grid gap-5 sm:grid-cols-2">
		<div>
			<label class="label" for="filter-allow">Toujours autoriser</label>
			<textarea
				id="filter-allow"
				class="field h-28"
				placeholder="lumni.fr"
				autocapitalize="off"
				value={filter.allow.join('\n')}
				onchange={(e) => (filter.allow = lines(e.currentTarget.value))}
			></textarea>
			<p class="hint">Un site par ligne. Ils passent même s'ils figurent dans une catégorie bloquée.</p>
		</div>
		<div>
			<label class="label" for="filter-deny">Toujours bloquer</label>
			<textarea
				id="filter-deny"
				class="field h-28"
				placeholder="exemple.com"
				autocapitalize="off"
				value={filter.deny.join('\n')}
				onchange={(e) => (filter.deny = lines(e.currentTarget.value))}
			></textarea>
			<p class="hint">Un site par ligne. Les sous-domaines sont inclus.</p>
		</div>
	</section>
</div>
