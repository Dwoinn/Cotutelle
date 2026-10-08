<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '#lib/api.ts';
	import { attempt, catalog as loadCatalog } from '#lib/app.svelte.ts';
	import Avatar from '#lib/components/Avatar.svelte';
	import FilterEditor from '#lib/components/FilterEditor.svelte';
	import ScheduleEditor from '#lib/components/ScheduleEditor.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import Switch from '#lib/components/Switch.svelte';
	import WeekChart from '#lib/components/WeekChart.svelte';
	import { minutes, seconds } from '#lib/format.ts';
	import { scheduleValid } from '#lib/schedule.ts';
	import type { Activity, Catalog, Child } from '#lib/types.ts';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Eye from '@lucide/svelte/icons/eye';

	const COLORS = ['#6366f1', '#f59e0b', '#10b981', '#ec4899', '#0ea5e9', '#8b5cf6', '#ef4444', '#14b8a6'];
	type Tab = 'activite' | 'temps' | 'filtre' | 'profil';

	const id = $derived(page.params.id);
	let tab = $state<Tab>('activite');
	let saved = $state<Child | null>(null);
	let draft = $state<Child | null>(null);
	let catalog = $state<Catalog | null>(null);
	let activity = $state<Activity | null>(null);
	let days = $state(7);
	let confirmDelete = $state(false);

	const dirty = $derived(saved !== null && draft !== null && JSON.stringify(saved) !== JSON.stringify(draft));
	const valid = $derived(draft !== null && draft.name.trim().length > 0 && scheduleValid(draft.policy.schedule));
	const thisYear = new Date().getFullYear();

	async function load(childId: string) {
		const child = await attempt(() => api.get<Child>(`/children/${childId}`));
		if (!child) return;
		saved = child;
		draft = structuredClone(child);
		catalog = (await attempt(loadCatalog)) ?? null;
	}

	async function loadActivity(childId: string, span: number) {
		activity = (await attempt(() => api.get<Activity>(`/children/${childId}/activity?days=${span}`))) ?? null;
	}

	$effect(() => {
		if (id) load(id);
	});
	$effect(() => {
		if (id) loadActivity(id, days);
	});

	async function save() {
		if (!draft) return;
		const child = await attempt(() => api.put<Child>(`/children/${draft!.id}`, $state.snapshot(draft)), 'Enregistré');
		if (child) {
			saved = child;
			draft = structuredClone(child);
		}
	}

	function cancel() {
		if (saved) draft = structuredClone(saved);
	}

	async function remove() {
		if (!saved) return;
		if (await attempt(() => api.del(`/children/${saved!.id}`), `${saved.name} supprimé`)) goto('/');
	}

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
</script>

{#if draft && saved}
	<div class="mx-auto max-w-2xl space-y-5 pb-24">
		<div class="flex items-center gap-3">
			<a href="/" class="icon-btn bg-surface" aria-label="Retour à l'accueil"><ArrowLeft size={22} /></a>
			<Avatar name={draft.name} color={draft.color} size={52} />
			<div class="min-w-0 flex-1">
				<h1 class="truncate">{saved.name}</h1>
				{#if saved.birth_year}<p class="muted text-sm">{thisYear - saved.birth_year} ans</p>{/if}
			</div>
			<a class="icon-btn bg-surface" href="/moi?apercu={saved.id}" target="_blank" rel="noopener" aria-label="Voir l'espace de {saved.name}" title="Voir son espace">
				<Eye size={22} />
			</a>
		</div>

		<Segmented
			bind:value={tab}
			label="Rubrique"
			options={[
				{ value: 'activite', label: 'Activité' },
				{ value: 'temps', label: 'Temps' },
				{ value: 'filtre', label: 'Filtre' },
				{ value: 'profil', label: 'Profil' }
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
					<WeekChart days={activity.days} quota={saved.policy.quota.daily_minutes} />
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
				<h2 class="mb-1">Temps d'écran</h2>
				<div class="rows">
					<div>
						<Switch
							checked={draft.policy.quota.daily_minutes !== null}
							label="Limiter le temps par jour"
							description="Partagé entre tous ses appareils."
							onchange={(on) => (draft!.policy.quota.daily_minutes = on ? 90 : null)}
						/>
						{#if draft.policy.quota.daily_minutes !== null}
							<div class="pb-4">
								<p class="display mb-2 text-4xl">{minutes(draft.policy.quota.daily_minutes)}</p>
								<input type="range" min="15" max="360" step="15" class="w-full accent-[var(--sun)]" aria-label="Minutes par jour" bind:value={draft.policy.quota.daily_minutes} />
							</div>
						{/if}
					</div>
					<div>
						<Switch
							checked={draft.policy.quota.weekly_minutes !== null}
							label="Limiter aussi le temps par semaine"
							description="Du lundi au dimanche."
							onchange={(on) => (draft!.policy.quota.weekly_minutes = on ? 600 : null)}
						/>
						{#if draft.policy.quota.weekly_minutes !== null}
							<div class="pb-4">
								<p class="display mb-2 text-4xl">{minutes(draft.policy.quota.weekly_minutes)}</p>
								<input type="range" min="60" max="2400" step="30" class="w-full accent-[var(--sun)]" aria-label="Minutes par semaine" bind:value={draft.policy.quota.weekly_minutes} />
							</div>
						{/if}
					</div>
				</div>
			</section>

			<section class="panel">
				<h2>Horaires</h2>
				<p class="muted mb-2 text-sm">En dehors de ces plages, la session se verrouille, après un rappel à 5 minutes puis à 1 minute.</p>
				<ScheduleEditor bind:schedule={draft.policy.schedule} />
			</section>
		{:else if tab === 'filtre'}
			<section class="panel py-2">
				<Switch
					bind:checked={draft.policy.filter.allow_requests}
					label="{saved.name} peut faire des demandes"
					description="Depuis son espace : obtenir du temps, ouvrir un site ou l'un des services réglés « sur demande ». Vous répondez depuis l'accueil."
				/>
			</section>
			<section class="panel">
				{#if catalog}
					<FilterEditor bind:filter={draft.policy.filter} bind:catalog childName={saved.name} />
				{/if}
			</section>
		{:else}
			<section class="panel space-y-5">
				<div class="grid gap-4 sm:grid-cols-2">
					<div>
						<label class="label" for="name">Prénom</label>
						<input id="name" class="field" bind:value={draft.name} maxlength="40" />
					</div>
					<div>
						<label class="label" for="year">Année de naissance</label>
						<input id="year" type="number" inputmode="numeric" class="field" bind:value={draft.birth_year} min={thisYear - 25} max={thisYear} />
					</div>
				</div>
				<div>
					<p class="label">Couleur</p>
					<div class="flex flex-wrap gap-3">
						{#each COLORS as color (color)}
							<button
								class="h-12 w-12 rounded-2xl transition-transform {draft.color === color ? 'scale-110 outline-3 outline-offset-2 outline-[var(--ink)]' : ''}"
								style:background={color}
								aria-label="Couleur {color}"
								aria-pressed={draft.color === color}
								onclick={() => (draft!.color = color)}
							></button>
						{/each}
					</div>
				</div>
			</section>

			<section class="panel">
				<h2>Supprimer {saved.name}</h2>
				<p class="muted mt-1 mb-4 text-sm">Ses réglages, son historique et ses rattachements d'appareils seront effacés.</p>
				{#if confirmDelete}
					<div class="grid gap-2 sm:grid-cols-2">
						<button class="btn-danger" onclick={remove}>Supprimer définitivement</button>
						<button class="btn-quiet" onclick={() => (confirmDelete = false)}>Annuler</button>
					</div>
				{:else}
					<button class="btn-danger" onclick={() => (confirmDelete = true)}>Supprimer</button>
				{/if}
			</section>
		{/if}
	</div>

	{#if dirty}
		<div class="fixed inset-x-0 bottom-[calc(4.75rem+env(safe-area-inset-bottom))] z-30 px-3 sm:bottom-6">
			<div class="sheet bg-night mx-auto flex max-w-md items-center gap-2 rounded-[22px] p-2 pl-4 text-white shadow-xl">
				<span class="min-w-0 flex-1 text-sm font-semibold">{valid ? 'Modifications à enregistrer' : 'Une plage horaire est invalide'}</span>
				<button class="btn btn-sm text-white/70 hover:text-white" onclick={cancel}>Annuler</button>
				<button class="btn-sun btn-sm" disabled={!valid} onclick={save}>Enregistrer</button>
			</div>
		</div>
	{/if}
{/if}
