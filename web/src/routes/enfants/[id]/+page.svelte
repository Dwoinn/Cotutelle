<script lang="ts">
	import { page } from '$app/state';
	import { api } from '#lib/api.ts';
	import { attempt, catalog as loadCatalog } from '#lib/app.svelte.ts';
	import Avatar from '#lib/components/Avatar.svelte';
	import FilterActions from '#lib/components/FilterActions.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import TimeActions from '#lib/components/TimeActions.svelte';
	import WeekChart from '#lib/components/WeekChart.svelte';
	import { filterGrants, nowMinutes, seconds, timeGrants, timeLine } from '#lib/format.ts';
	import type { Activity, Catalog, Child, ChildSummary } from '#lib/types.ts';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Eye from '@lucide/svelte/icons/eye';
	import Settings from '@lucide/svelte/icons/settings';
	import { onMount } from 'svelte';

	// Fiche d'un enfant : son activité et l'état du moment, avec les mêmes
	// mesures temporaires que l'accueil. Les réglages durables sont dans
	// la page Paramètres.
	type Tab = 'activite' | 'temps' | 'filtre';
	type Summary = { child: ChildSummary; now: number };

	const id = $derived(page.params.id);
	let tab = $state<Tab>('activite');
	let child = $state<Child | null>(null);
	let summary = $state<Summary | null>(null);
	let catalog = $state<Catalog | null>(null);
	let activity = $state<Activity | null>(null);
	let days = $state(7);
	let minute = $state(nowMinutes());

	const thisYear = new Date().getFullYear();

	async function load(childId: string) {
		child = (await attempt(() => api.get<Child>(`/children/${childId}`))) ?? null;
		catalog = (await attempt(loadCatalog)) ?? null;
	}

	async function refresh(childId: string) {
		const next = await attempt(() => api.get<Summary>(`/children/${childId}/summary`));
		if (next) {
			summary = next;
			minute = nowMinutes();
		}
	}

	async function loadActivity(childId: string, span: number) {
		activity = (await attempt(() => api.get<Activity>(`/children/${childId}/activity?days=${span}`))) ?? null;
	}

	$effect(() => {
		if (id) {
			load(id);
			refresh(id);
		}
	});
	$effect(() => {
		if (id) loadActivity(id, days);
	});
	onMount(() => {
		// L'état du moment change sans cesse (temps consommé, mesures) : on rafraîchit.
		const timer = setInterval(() => {
			if (id) refresh(id);
		}, 10_000);
		return () => clearInterval(timer);
	});

	function reasonLabel(reason: { kind: string; id?: string } | null): string {
		if (!reason) return '';
		if (reason.kind === 'service') return catalog?.services.find((s) => s.id === reason.id)?.label ?? reason.id ?? '';
		if (reason.kind === 'category') {
			const all = catalog?.groups.flatMap((g) => g.categories) ?? [];
			return all.find((c) => c.id === reason.id)?.label ?? reason.id ?? '';
		}
		if (reason.kind === 'deny_list') return 'Liste personnelle';
		if (reason.kind === 'paused') return 'Accès fermé';
		return '';
	}

	const maxAllowed = $derived(Math.max(1, ...(activity?.top_domains.map((d) => d.allowed) ?? [1])));
	const blockedTotal = $derived(activity?.blocked.reduce((sum, b) => sum + b.blocked, 0) ?? 0);
	const line = $derived(summary ? timeLine(summary.child.status, summary.child.today_ranges ?? [], summary.child.next_opening, minute) : '');
</script>

{#if child}
	<div class="mx-auto max-w-2xl space-y-5 pb-24">
		<div class="flex items-center gap-3">
			<a href="/" class="icon-btn bg-surface" aria-label="Retour à l'accueil"><ArrowLeft size={22} /></a>
			<Avatar name={child.name} color={child.color} size={52} />
			<div class="min-w-0 flex-1">
				<h1 class="truncate">{child.name}</h1>
				{#if child.birth_year}<p class="muted text-sm">{thisYear - child.birth_year} ans</p>{/if}
			</div>
			<a class="icon-btn bg-surface" href="/moi?apercu={child.id}" target="_blank" rel="noopener" aria-label="Voir l'espace de {child.name}" title="Voir son espace">
				<Eye size={22} />
			</a>
			<a class="icon-btn bg-surface" href="/enfants/{child.id}/parametres" aria-label="Paramètres de {child.name}" title="Paramètres">
				<Settings size={22} />
			</a>
		</div>

		<Segmented
			bind:value={tab}
			label="Rubrique"
			options={[
				{ value: 'activite', label: 'Activité' },
				{ value: 'temps', label: 'Temps' },
				{ value: 'filtre', label: 'Filtre' }
			]}
		/>

		{#if tab === 'activite'}
			<section class="panel">
				<div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<h2>Temps d'écran</h2>
					<div class="sm:w-48">
						<Segmented
							bind:value={days}
							label="Période"
							options={[
								{ value: 7, label: '7 jours' },
								{ value: 30, label: '30 jours' }
							]}
						/>
					</div>
				</div>
				{#if activity}
					<WeekChart days={activity.days} quota={child.policy.quota.daily_minutes} />
					{#if activity.devices.length > 1}
						<div class="rows border-line mt-4 border-t">
							{#each activity.devices as device (device.id)}
								<div class="row min-h-12 justify-between py-1">
									<span>{device.name}</span>
									<span class="display text-lg">{seconds(device.seconds)}</span>
								</div>
							{/each}
						</div>
					{/if}
				{/if}
			</section>

			<section class="panel">
				<h2>Sites consultés</h2>
				<p class="muted mb-3 text-sm">Le nom des sites seulement. Ni les pages, ni les recherches ne sont enregistrées.</p>
				{#if activity && activity.top_domains.length > 0}
					<ul class="space-y-2.5">
						{#each activity.top_domains.slice(0, 12) as site (site.domain)}
							<li>
								<span class="flex items-baseline justify-between gap-3 text-[0.95rem]">
									<span class="truncate">{site.domain}</span>
									<span class="muted text-sm tabular-nums">{site.allowed}</span>
								</span>
								<span class="bg-sunken mt-1 block h-1.5 overflow-hidden rounded-full">
									<span class="bg-ink block h-full rounded-full" style:width="{(site.allowed / maxAllowed) * 100}%"></span>
								</span>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="muted">Rien à afficher pour l'instant.</p>
				{/if}
			</section>
		{:else if tab === 'temps'}
			<section class="panel">
				<h2 class="mb-4">Temps de {child.name}</h2>
				{#if summary}
					<TimeActions
						name={child.name}
						childId={child.id}
						status={summary.child.status}
						{line}
						grants={timeGrants(summary.child.grants)}
						now={summary.now}
						ondone={() => refresh(child!.id)}
					/>
				{/if}
			</section>
		{:else}
			<section class="panel">
				<h2 class="mb-4">Filtre de {child.name}</h2>
				{#if summary && catalog}
					<FilterActions
						name={child.name}
						subject={{ child_id: child.id }}
						services={catalog.services}
						blockedServices={summary.child.blocked_services}
						requestableServices={summary.child.requestable_services}
						grants={filterGrants(summary.child.grants)}
						now={summary.now}
						blockedToday={summary.child.blocked_today ?? 0}
						ondone={() => refresh(child!.id)}
					/>
				{/if}
			</section>

			<section class="panel">
				<h2>Ce que le filtre a bloqué</h2>
				<p class="muted mb-2 text-sm">
					{#if blockedTotal > 0}{blockedTotal} tentatives sur {days} jours, avec le motif.{:else}Rien sur {days} jours.{/if}
				</p>
				{#if activity && activity.blocked.length > 0}
					<div class="rows">
						{#each activity.blocked.slice(0, 12) as site (site.domain)}
							<div class="row min-h-12 py-1.5">
								<span class="min-w-0 flex-1">
									<span class="block truncate font-semibold">{site.domain}</span>
									{#if reasonLabel(site.reason)}<span class="muted block text-sm">{reasonLabel(site.reason)}</span>{/if}
								</span>
								<span class="display text-lg">{site.blocked}</span>
							</div>
						{/each}
					</div>
				{/if}
			</section>
		{/if}
	</div>
{/if}
