<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '#lib/api.ts';
	import { attempt, catalog as loadCatalog } from '#lib/app.svelte.ts';
	import FilterEditor from '#lib/components/FilterEditor.svelte';
	import ScheduleEditor from '#lib/components/ScheduleEditor.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import WeekChart from '#lib/components/WeekChart.svelte';
	import { minutes, seconds } from '#lib/format.ts';
	import { scheduleValid } from '#lib/schedule.ts';
	import type { Activity, Catalog, Child } from '#lib/types.ts';

	const EMOJIS = ['🦊', '🐼', '🦁', '🐙', '🦄', '🐢', '🐱', '🐶', '🚀', '⚽', '🎨', '🎸', '🌈', '🦖', '🧙', '🤖'];
	const COLORS = ['#6366f1', '#f59e0b', '#10b981', '#ec4899', '#0ea5e9', '#8b5cf6', '#ef4444', '#14b8a6'];
	const TABS = [
		{ id: 'activite', label: 'Activité' },
		{ id: 'temps', label: 'Horaires et temps' },
		{ id: 'filtres', label: 'Filtres' },
		{ id: 'profil', label: 'Profil' }
	] as const;

	const id = $derived(page.params.id);
	let tab = $state<(typeof TABS)[number]['id']>('activite');
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
</script>

{#if !draft || !saved}
	<p class="muted">Chargement…</p>
{:else}
	<div class="space-y-5 pb-20">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex items-center gap-3">
				<a href="/" class="btn-ghost btn-sm" aria-label="Retour">←</a>
				<span
					class="flex h-12 w-12 items-center justify-center rounded-2xl text-2xl"
					style:background="{draft.color}22"
				>
					{draft.emoji}
				</span>
				<div>
					<h1>{saved.name}</h1>
					{#if saved.birth_year}<p class="muted">{thisYear - saved.birth_year} ans</p>{/if}
				</div>
			</div>
			<a class="btn-soft" href="/moi?apercu={saved.id}" target="_blank" rel="noopener">👀 Voir son espace</a>
		</div>

		<div class="flex gap-1 overflow-x-auto rounded-full bg-slate-100 p-1 dark:bg-slate-900" role="tablist">
			{#each TABS as t (t.id)}
				<button
					role="tab"
					aria-selected={tab === t.id}
					class="flex-1 rounded-full px-4 py-2 text-sm font-semibold whitespace-nowrap transition {tab === t.id
						? 'bg-white shadow-sm dark:bg-slate-800'
						: 'text-slate-500 hover:text-slate-700 dark:hover:text-slate-300'}"
					onclick={() => (tab = t.id)}
				>
					{t.label}
				</button>
			{/each}
		</div>

		{#if tab === 'activite'}
			<div class="card space-y-4">
				<div class="flex items-center justify-between">
					<h2>Temps d'écran</h2>
					<div class="flex gap-1">
						{#each [7, 30] as span (span)}
							<button class={days === span ? 'btn-soft btn-sm' : 'btn-ghost btn-sm'} onclick={() => (days = span)}>
								{span} jours
							</button>
						{/each}
					</div>
				</div>
				{#if activity}
					<WeekChart days={activity.days} quota={saved.policy.quota.daily_minutes} color={saved.color} />
					{#if activity.devices.length > 1}
						<div class="flex flex-wrap gap-2">
							{#each activity.devices as device (device.id)}
								<span class="chip">{device.name} · {seconds(device.seconds)}</span>
							{/each}
						</div>
					{/if}
				{/if}
			</div>

			<div class="grid gap-4 md:grid-cols-2">
				<div class="card">
					<h2>Sites les plus consultés</h2>
					<p class="muted mb-3 text-xs">Par nombre de requêtes, sur {days} jours. Les pages visitées ne sont pas enregistrées.</p>
					{#if activity && activity.top_domains.length > 0}
						<ul class="space-y-1.5">
							{#each activity.top_domains.slice(0, 12) as site (site.domain)}
								<li class="relative overflow-hidden rounded-lg px-2 py-1 text-sm">
									<span
										class="absolute inset-y-0 left-0 rounded-lg opacity-15"
										style:width="{(site.allowed / maxAllowed) * 100}%"
										style:background={saved.color}
									></span>
									<span class="relative flex justify-between gap-2">
										<span class="truncate">{site.domain}</span>
										<span class="muted text-xs tabular-nums">{site.allowed}</span>
									</span>
								</li>
							{/each}
						</ul>
					{:else}
						<p class="muted">Rien à afficher pour l'instant.</p>
					{/if}
				</div>

				<div class="card">
					<h2>Tentatives bloquées</h2>
					<p class="muted mb-3 text-xs">Sites que le filtre a refusés, avec le motif.</p>
					{#if activity && activity.blocked.length > 0}
						<ul class="space-y-1.5">
							{#each activity.blocked.slice(0, 12) as site (site.domain)}
								<li class="flex items-center justify-between gap-2 px-2 py-1 text-sm">
									<span class="truncate">{site.domain}</span>
									<span class="flex shrink-0 items-center gap-2">
										{#if reasonLabel(site.reason)}<span class="chip">{reasonLabel(site.reason)}</span>{/if}
										<span class="muted text-xs tabular-nums">{site.blocked}×</span>
									</span>
								</li>
							{/each}
						</ul>
					{:else}
						<p class="muted">Aucune tentative bloquée.</p>
					{/if}
				</div>
			</div>
		{:else if tab === 'temps'}
			<div class="card space-y-4">
				<h2>Temps d'écran</h2>
				<Toggle
					checked={draft.policy.quota.daily_minutes !== null}
					label="Limiter le temps par jour"
					description="Le temps est partagé entre tous ses appareils."
					onchange={(on) => (draft!.policy.quota.daily_minutes = on ? 90 : null)}
				/>
				{#if draft.policy.quota.daily_minutes !== null}
					<div class="flex items-center gap-4">
						<input
							type="range"
							min="15"
							max="360"
							step="15"
							class="accent-brand-500 flex-1"
							aria-label="Minutes par jour"
							bind:value={draft.policy.quota.daily_minutes}
						/>
						<span class="w-20 text-right text-lg font-bold tabular-nums">{minutes(draft.policy.quota.daily_minutes)}</span>
					</div>
				{/if}
				<Toggle
					checked={draft.policy.quota.weekly_minutes !== null}
					label="Limiter aussi le temps par semaine"
					description="Du lundi au dimanche."
					onchange={(on) => (draft!.policy.quota.weekly_minutes = on ? 600 : null)}
				/>
				{#if draft.policy.quota.weekly_minutes !== null}
					<div class="flex items-center gap-4">
						<input
							type="range"
							min="60"
							max="2400"
							step="30"
							class="accent-brand-500 flex-1"
							aria-label="Minutes par semaine"
							bind:value={draft.policy.quota.weekly_minutes}
						/>
						<span class="w-20 text-right text-lg font-bold tabular-nums">{minutes(draft.policy.quota.weekly_minutes)}</span>
					</div>
				{/if}
			</div>

			<div class="card space-y-3">
				<div>
					<h2>Horaires autorisés</h2>
					<p class="muted">
						En dehors de ces plages, la session se verrouille après un préavis de 5 minutes puis d'une minute.
					</p>
				</div>
				<ScheduleEditor bind:schedule={draft.policy.schedule} />
			</div>
		{:else if tab === 'filtres'}
			<div class="card space-y-4">
				<Toggle
					bind:checked={draft.policy.filter.allow_requests}
					label="{saved.name} peut faire des demandes"
					description="Depuis son espace : débloquer un service ou un site, obtenir du temps en plus. Vous validez depuis l'accueil."
				/>
			</div>
			<div class="card">
				{#if catalog}
					<FilterEditor bind:filter={draft.policy.filter} {catalog} />
				{:else}
					<p class="muted">Chargement du catalogue…</p>
				{/if}
			</div>
		{:else}
			<div class="card space-y-4">
				<div class="grid gap-4 sm:grid-cols-2">
					<div>
						<label class="label" for="name">Prénom</label>
						<input id="name" class="input" bind:value={draft.name} maxlength="40" />
					</div>
					<div>
						<label class="label" for="year">Année de naissance</label>
						<input id="year" type="number" class="input" bind:value={draft.birth_year} min={thisYear - 25} max={thisYear} />
					</div>
				</div>
				<div>
					<p class="label">Avatar</p>
					<div class="flex flex-wrap gap-2">
						{#each EMOJIS as emoji (emoji)}
							<button
								class="h-11 w-11 rounded-2xl text-2xl transition {draft.emoji === emoji
									? 'bg-brand-100 ring-brand-500 dark:bg-brand-900/50 ring-2'
									: 'bg-slate-100 hover:bg-slate-200 dark:bg-slate-800'}"
								aria-pressed={draft.emoji === emoji}
								onclick={() => (draft!.emoji = emoji)}
							>
								{emoji}
							</button>
						{/each}
					</div>
				</div>
				<div>
					<p class="label">Couleur</p>
					<div class="flex flex-wrap gap-2">
						{#each COLORS as color (color)}
							<button
								class="h-9 w-9 rounded-full transition {draft.color === color
									? 'ring-2 ring-slate-900 ring-offset-2 dark:ring-white dark:ring-offset-slate-900'
									: ''}"
								style:background={color}
								aria-label="Couleur {color}"
								aria-pressed={draft.color === color}
								onclick={() => (draft!.color = color)}
							></button>
						{/each}
					</div>
				</div>
			</div>

			<div class="card border-rose-200 dark:border-rose-900/60">
				<h2>Supprimer {saved.name}</h2>
				<p class="muted mb-3">Ses réglages, son historique et ses rattachements d'appareils seront effacés.</p>
				{#if confirmDelete}
					<div class="flex gap-2">
						<button class="btn-danger" onclick={remove}>Oui, supprimer définitivement</button>
						<button class="btn-ghost" onclick={() => (confirmDelete = false)}>Annuler</button>
					</div>
				{:else}
					<button class="btn-danger" onclick={() => (confirmDelete = true)}>Supprimer…</button>
				{/if}
			</div>
		{/if}
	</div>

	{#if dirty}
		<div class="fixed inset-x-0 bottom-20 z-30 flex justify-center px-4 sm:bottom-6">
			<div class="flex items-center gap-3 rounded-full bg-slate-900 py-2 pr-2 pl-5 text-white shadow-xl dark:bg-slate-700">
				<span class="text-sm">{valid ? 'Modifications non enregistrées' : 'Une plage horaire est invalide'}</span>
				<button class="btn btn-sm text-slate-300 hover:text-white" onclick={cancel}>Annuler</button>
				<button class="btn-primary btn-sm" disabled={!valid} onclick={save}>Enregistrer</button>
			</div>
		</div>
	{/if}
{/if}
