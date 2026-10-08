<script lang="ts">
	import { type ServiceMode as Mode, serviceMode, setServiceMode, siteCount } from '#lib/services.ts';
	import type { Catalog, Policy, Service } from '#lib/types.ts';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Plus from '@lucide/svelte/icons/plus';
	import ServiceLogo from './ServiceLogo.svelte';
	import ServiceMode from './ServiceMode.svelte';
	import ServiceSheet from './ServiceSheet.svelte';
	import Shield from './Shield.svelte';
	import Switch from './Switch.svelte';

	// Réglage du filtre : services, catégories de sites, listes personnelles.
	let {
		filter = $bindable(),
		catalog = $bindable(),
		childName = null
	}: {
		filter: Policy['filter'];
		catalog: Catalog;
		/** L'enfant concerné ; absent pour un appareil partagé, où personne ne fait de demande. */
		childName?: string | null;
	} = $props();

	const fmt = new Intl.NumberFormat('fr-FR');

	// Les positions de l'interrupteur, expliquées une fois au-dessus de la liste.
	const KEY = $derived(
		(
			[
				{ mode: 'open', term: 'Autorisé', meaning: 'le service n’est pas bloqué.' },
				{ mode: 'ask', term: 'Sur demande', meaning: `bloqué, mais ${childName} peut vous demander de l’ouvrir.` },
				{
					mode: 'blocked',
					term: 'Bloqué',
					meaning: childName ? `bloqué, sans être proposé à ${childName}.` : 'vous pouvez l’ouvrir pour un moment depuis l’accueil.'
				}
			] satisfies { mode: Mode; term: string; meaning: string }[]
		).filter((k) => childName || k.mode !== 'ask')
	);

	let editing = $state<Service | null>(null);
	let sheetOpen = $state(false);

	function edit(service: Service | null) {
		editing = service;
		sheetOpen = true;
	}

	function saved(service: Service, added: boolean) {
		const at = catalog.services.findIndex((s) => s.id === service.id);
		catalog.services = at < 0 ? [...catalog.services, service] : catalog.services.with(at, service);
		// Un service qu'on vient d'ajouter ici, c'est pour le bloquer.
		if (added) setServiceMode(filter, service.id, childName ? 'ask' : 'blocked');
	}

	function removed(id: string) {
		catalog.services = catalog.services.filter((s) => s.id !== id);
		setServiceMode(filter, id, 'open');
	}

	function toggle(id: string) {
		const blocked = filter.blocked_categories;
		filter.blocked_categories = blocked.includes(id) ? blocked.filter((x) => x !== id) : [...blocked, id];
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
		<p class="muted mb-3 text-sm">
			Un service réunit tous les sites dont une appli a besoin. Touchez son nom pour les voir ou en ajouter.
		</p>
		<ul class="bg-bg mb-1 space-y-1.5 rounded-[20px] px-3.5 py-3 text-sm">
			{#each KEY as key (key.mode)}
				<li class="flex items-start gap-2.5">
					<Shield size={22} opened={key.mode === 'ask'} inactive={key.mode === 'open'} />
					<span><strong class="font-semibold">{key.term}</strong><span class="muted">&nbsp;: {key.meaning}</span></span>
				</li>
			{/each}
		</ul>
		<div class="rows">
			{#each catalog.services as service (service.id)}
				<div class="row gap-2 py-1.5">
					<button type="button" class="-my-1 flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-xl text-left" onclick={() => edit(service)}>
						<ServiceLogo label={service.label} logo={service.logo} size={38} />
						<span class="min-w-0">
							<span class="block leading-tight font-semibold [overflow-wrap:anywhere]">{service.label}</span>
							<span class="muted block text-sm">{siteCount(service.domains)}</span>
						</span>
					</button>
					<ServiceMode
						value={serviceMode(filter, service.id)}
						name={service.label}
						ask={childName !== null}
						onchange={(mode) => setServiceMode(filter, service.id, mode)}
					/>
				</div>
			{/each}
			<button type="button" class="row gap-3" onclick={() => edit(null)}>
				<span class="border-line text-muted flex h-[38px] w-[38px] shrink-0 items-center justify-center rounded-[11px] border-2 border-dashed">
					<Plus size={20} />
				</span>
				<span class="font-semibold">Ajouter un service</span>
			</button>
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
								<button type="button" class="row justify-between" aria-pressed={blocked} onclick={() => toggle(category.id)}>
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

<ServiceSheet bind:open={sheetOpen} service={editing} onsaved={saved} onremoved={removed} />
