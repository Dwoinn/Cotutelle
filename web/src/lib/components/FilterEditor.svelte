<script lang="ts">
	import type { Catalog, Policy } from '#lib/types.ts';
	import Toggle from './Toggle.svelte';

	// Éditeur des filtres : services, catégories de listes, listes personnelles.
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

<div class="space-y-6">
	{#if !catalog.blocklists_ready}
		<p class="pill-warn rounded-2xl px-4 py-3 text-sm">
			Les listes de blocage ne sont pas encore téléchargées. Les catégories s'appliqueront dès leur arrivée ; les
			services et les listes personnelles fonctionnent déjà.
		</p>
	{/if}

	<section>
		<h3 class="font-semibold">Services</h3>
		<p class="muted mb-3">Touchez un service pour le bloquer ou l'autoriser. Un service bloqué peut être ouvert ponctuellement.</p>
		<div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
			{#each catalog.services as service (service.id)}
				{@const blocked = filter.blocked_services.includes(service.id)}
				<button
					type="button"
					aria-pressed={blocked}
					onclick={() => toggle('blocked_services', service.id)}
					class="flex items-center gap-2 rounded-2xl border px-3 py-2.5 text-left text-sm transition {blocked
						? 'border-rose-200 bg-rose-50 text-rose-800 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-200'
						: 'border-slate-200 hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800'}"
				>
					<span class="text-lg {blocked ? 'grayscale' : ''}">{service.icon}</span>
					<span class="flex-1 font-medium">{service.label}</span>
					<span class="text-xs opacity-70">{blocked ? 'bloqué' : 'autorisé'}</span>
				</button>
			{/each}
		</div>
		<div class="mt-3">
			<Toggle
				bind:checked={filter.youtube_restricted}
				label="Mode restreint de YouTube"
				description="Quand YouTube est accessible, masque les vidéos signalées comme inadaptées."
			/>
		</div>
	</section>

	<section>
		<h3 class="font-semibold">Catégories de sites</h3>
		<p class="muted mb-3">
			Listes de l'Université Toulouse Capitole, mises à jour chaque jour.
		</p>
		<div class="space-y-2">
			{#each catalog.groups as group (group.id)}
				{@const ids = group.categories.map((c) => c.id)}
				{@const count = ids.filter((id) => filter.blocked_categories.includes(id)).length}
				<details class="rounded-2xl border border-slate-200 dark:border-slate-800" open={group.id === 'loisirs'}>
					<summary class="flex items-center justify-between gap-3 px-4 py-3 text-sm font-medium">
						<span>{group.label}</span>
						<span class={count === ids.length ? 'pill-ok' : count === 0 ? 'pill-off' : 'pill-warn'}>
							{count} / {ids.length} bloquées
						</span>
					</summary>
					<div class="border-t border-slate-100 px-4 py-3 dark:border-slate-800">
						<div class="mb-2 flex gap-2">
							<button class="btn-ghost btn-sm" onclick={() => setGroup(ids, true)}>Tout bloquer</button>
							<button class="btn-ghost btn-sm" onclick={() => setGroup(ids, false)}>Tout autoriser</button>
						</div>
						<div class="grid gap-x-6 gap-y-1 sm:grid-cols-2">
							{#each group.categories as category (category.id)}
								<label class="flex cursor-pointer items-start gap-2.5 rounded-xl px-1 py-1.5 text-sm">
									<input
										type="checkbox"
										class="accent-brand-500 mt-0.5 h-4 w-4"
										checked={filter.blocked_categories.includes(category.id)}
										onchange={() => toggle('blocked_categories', category.id)}
									/>
									<span>
										<span class="font-medium">{category.label}</span>
										{#if category.recommended}<span class="muted text-xs"> · conseillé</span>{/if}
										<span class="muted block text-xs">
											{category.description}{#if category.entries}
												· {fmt.format(category.entries)} sites{/if}
										</span>
									</span>
								</label>
							{/each}
						</div>
					</div>
				</details>
			{/each}
		</div>
	</section>

	<section class="grid gap-4 sm:grid-cols-2">
		<div>
			<label class="label" for="filter-allow">Toujours autoriser</label>
			<textarea
				id="filter-allow"
				class="input h-28 font-mono text-xs"
				placeholder={'un site par ligne\nlumni.fr'}
				value={filter.allow.join('\n')}
				onchange={(e) => (filter.allow = lines(e.currentTarget.value))}
			></textarea>
			<p class="muted text-xs">Ces sites passent même s'ils figurent dans une catégorie bloquée.</p>
		</div>
		<div>
			<label class="label" for="filter-deny">Toujours bloquer</label>
			<textarea
				id="filter-deny"
				class="input h-28 font-mono text-xs"
				placeholder={'un site par ligne\nexemple.com'}
				value={filter.deny.join('\n')}
				onchange={(e) => (filter.deny = lines(e.currentTarget.value))}
			></textarea>
			<p class="muted text-xs">Les sous-domaines sont inclus.</p>
		</div>
	</section>
</div>
