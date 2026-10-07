<script lang="ts">
	import { goto } from '$app/navigation';
	import { ApiError, api } from '#lib/api.ts';
	import { app, attempt, catalog } from '#lib/app.svelte.ts';
	import Avatar from '#lib/components/Avatar.svelte';
	import Dial from '#lib/components/Dial.svelte';
	import FilterSheet from '#lib/components/FilterSheet.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import Shield from '#lib/components/Shield.svelte';
	import TimeSheet from '#lib/components/TimeSheet.svelte';
	import { ago, filterGrants, grantRemaining, minutes, nowMinutes, targetLabel, timeGrants, timeLine } from '#lib/format.ts';
	import type { Child, ChildRequest, ChildSummary, Dashboard, Device, Grant, Service } from '#lib/types.ts';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Moon from '@lucide/svelte/icons/moon';
	import Pause from '@lucide/svelte/icons/pause';
	import Plus from '@lucide/svelte/icons/plus';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import { onMount } from 'svelte';

	let data = $state<Dashboard | null>(null);
	let services = $state<Service[]>([]);
	let failed = $state(false);
	let minute = $state(nowMinutes());

	// Feuilles ouvertes : on retient l'identifiant, les données restent vivantes.
	let timeId = $state<string | null>(null);
	let timeOpen = $state(false);
	let filterId = $state<string | null>(null);
	let filterOpen = $state(false);
	let deviceId = $state<string | null>(null);
	let deviceOpen = $state(false);
	let durationFor = $state<ChildRequest | null>(null);
	let durationOpen = $state(false);
	let duration = $state(30);
	let addOpen = $state(false);
	let newName = $state('');
	let newYear = $state<number | null>(null);

	async function load() {
		try {
			data = await api.get<Dashboard>('/dashboard');
			minute = nowMinutes();
			failed = false;
		} catch (error) {
			// Session expirée : retour à la page de connexion.
			if (error instanceof ApiError && error.status === 401) app.parent = null;
			failed = true;
		}
	}

	onMount(() => {
		load();
		catalog().then((c) => (services = c.services)).catch(() => {});
		// L'état change sans cesse (temps consommé, demandes) : on rafraîchit.
		const timer = setInterval(load, 10_000);
		return () => clearInterval(timer);
	});

	const shared = $derived(data?.devices.filter((d) => d.kind === 'network' && !d.child_id) ?? []);
	const timeChild = $derived(data?.children.find((c) => c.id === timeId) ?? null);
	const filterChild = $derived(data?.children.find((c) => c.id === filterId) ?? null);
	const sheetDevice = $derived(shared.find((d) => d.id === deviceId) ?? null);
	const thisYear = new Date().getFullYear();

	const deviceGrants = (device: Device) => data?.device_grants.filter((g) => g.device_id === device.id) ?? [];
	const line = (child: ChildSummary) => timeLine(child.status, child.today_ranges ?? [], child.next_opening, minute);

	function remaining(child: ChildSummary): number | null {
		const quota = child.status.quota_today_minutes;
		return quota ? Math.max(0, 1 - child.used_today_seconds / 60 / quota) : null;
	}

	function filterLine(blocked: number, grants: Grant[], now: number): string {
		const opened = filterGrants(grants);
		if (opened.length === 1) return `${targetLabel(opened[0].target, services)} ouvert, ${grantRemaining(opened[0], now)}`;
		if (opened.length > 1) return `${opened.length} ouvertures en cours`;
		return blocked === 0 ? 'Rien de bloqué aujourd’hui' : `${blocked > 1 ? 'bloqués' : 'bloqué'} aujourd’hui`;
	}

	function where(child: ChildSummary): { text: string; warn: boolean } {
		if (child.devices.length === 0) return { text: 'Aucun appareil rattaché', warn: true };
		const active = child.devices.find((d) => d.active);
		if (active) return { text: `Sur ${active.name} en ce moment`, warn: false };
		const agents = child.devices.filter((d) => d.kind === 'agent');
		if (agents.length > 0 && agents.every((d) => !d.online)) {
			return { text: `${agents[0].name} hors ligne`, warn: false };
		}
		return { text: 'Aucune session ouverte', warn: false };
	}

	async function approve(request: ChildRequest, m?: number) {
		const body = m ? { minutes: m } : {};
		if (await attempt(() => api.post(`/requests/${request.id}/approve`, body), `Accordé à ${request.child_name}`)) {
			durationOpen = false;
			load();
		}
	}

	async function deny(request: ChildRequest) {
		if (await attempt(() => api.post(`/requests/${request.id}/deny`), 'Demande refusée')) load();
	}

	async function ack(id: string) {
		if (await attempt(() => api.post(`/alerts/${id}/ack`))) load();
	}

	async function addChild(event: SubmitEvent) {
		event.preventDefault();
		const child = await attempt(() => api.post<Child>('/children', { name: newName, birth_year: newYear || null }));
		if (child) {
			addOpen = false;
			newName = '';
			newYear = null;
			goto(`/enfants/${child.id}`);
		}
	}
</script>

{#if failed && !data}
	<div class="panel mx-auto max-w-md text-center">
		<h2>Le serveur ne répond pas</h2>
		<p class="muted mt-1">Vérifiez qu'il est allumé et que vous êtes sur le même réseau.</p>
		<button class="btn-primary mt-4" onclick={load}>Réessayer</button>
	</div>
{:else if data}
	{@const now = data.now}
	<div class="space-y-8">
		<!-- Ce qui attend une réponse passe avant tout le reste. -->
		{#if data.requests.length > 0 || data.alerts.length > 0}
			<section class="max-w-2xl space-y-3" aria-label="À traiter">
				{#each data.requests as request (request.id)}
					<div class="bg-sun text-night rounded-[28px] p-5">
						<p class="display text-[1.35rem] leading-tight">
							{request.child_name} demande {request.label}{#if request.minutes}&nbsp;pendant {minutes(request.minutes)}{/if}
						</p>
						{#if request.message}<p class="mt-1.5">« {request.message} »</p>{/if}
						<p class="mt-1 text-sm opacity-70">{ago(request.created_at, now)}</p>
						<div class="mt-4 grid grid-cols-2 gap-2">
							<button class="btn bg-night text-white" onclick={() => approve(request)}>Accorder</button>
							<button class="btn bg-white/60 text-night" onclick={() => deny(request)}>Refuser</button>
						</div>
						{#if request.target?.kind !== 'extra_minutes'}
							<button
								class="mt-2 min-h-11 w-full text-sm font-semibold underline underline-offset-4"
								onclick={() => {
									durationFor = request;
									duration = request.minutes ?? 30;
									durationOpen = true;
								}}
							>
								Accorder une autre durée
							</button>
						{/if}
					</div>
				{/each}

				{#each data.alerts as alert (alert.id)}
					<div class="panel flex items-center gap-3 py-3">
						<TriangleAlert size={22} class="text-ember shrink-0" />
						<p class="min-w-0 flex-1 text-[0.95rem]">
							{alert.message}
							<span class="muted block text-sm">{ago(alert.created_at, now)}</span>
						</p>
						<button class="btn-quiet btn-sm" onclick={() => ack(alert.id)}>Vu</button>
					</div>
				{/each}
			</section>
		{/if}

		<section>
			<div class="mb-4 flex items-center justify-between gap-3">
				<h1>Les enfants</h1>
				{#if data.children.length > 0}
					<button class="btn-quiet btn-sm bg-surface" onclick={() => (addOpen = true)}><Plus size={18} /> Ajouter</button>
				{/if}
			</div>

			{#if data.children.length === 0}
				<div class="panel py-10 text-center">
					<h2>Ajoutez votre premier enfant</h2>
					<p class="muted mx-auto mt-2 max-w-sm">
						Chaque enfant a son temps d'écran, ses horaires et son filtre. Des réglages adaptés à son âge sont
						proposés, vous les ajusterez ensuite.
					</p>
					<button class="btn-primary mt-5" onclick={() => (addOpen = true)}><Plus size={20} /> Ajouter un enfant</button>
				</div>
			{:else}
				<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
					{#each data.children as child (child.id)}
						{@const place = where(child)}
						{@const opened = filterGrants(child.grants)}
						{@const paused = child.status.reason === 'paused'}
						<article class="panel min-w-0">
							<a href="/enfants/{child.id}" class="-m-1 flex items-center gap-3 rounded-2xl p-1">
								<Avatar name={child.name} color={child.color} />
								<span class="min-w-0 flex-1">
									<span class="display block truncate text-[1.45rem]">{child.name}</span>
									<span class="block truncate text-sm {place.warn ? 'text-ember font-semibold' : 'muted'}">{place.text}</span>
								</span>
								<ChevronRight size={22} class="text-muted shrink-0" />
							</a>

							<div class="mt-4 grid grid-cols-2 gap-3">
								<button class="instrument" onclick={() => { timeId = child.id; timeOpen = true; }} aria-label="Temps de {child.name} : {line(child)}">
									<Dial size={124} ranges={child.today_ranges ?? []} now={minute} remaining={remaining(child)} open={child.status.open} low={child.status.open && (child.status.remaining_minutes ?? 99) <= 5}>
										{#if child.status.open}
											{#if child.status.remaining_minutes === null}
												<span class="display text-xl">Libre</span>
											{:else}
												<span class="display text-[1.55rem]">{minutes(child.status.remaining_minutes)}</span>
											{/if}
										{:else if paused}
											<Pause size={30} />
										{:else}
											<Moon size={30} class="text-muted" />
										{/if}
									</Dial>
									<span class="mt-2 font-semibold">Temps</span>
									<span class="muted px-1 text-sm leading-snug">{line(child)}</span>
								</button>

								<button class="instrument" onclick={() => { filterId = child.id; filterOpen = true; }} aria-label="Filtre de {child.name}">
									<Shield size={124} opened={opened.length > 0} inactive={child.devices.length === 0}>
										<span class="display text-[1.9rem]">{child.blocked_today ?? 0}</span>
									</Shield>
									<span class="mt-2 font-semibold">Filtre</span>
									<span class="muted px-1 text-sm leading-snug">{filterLine(child.blocked_today ?? 0, child.grants, now)}</span>
								</button>
							</div>
						</article>
					{/each}
				</div>
			{/if}
		</section>

		{#if shared.length > 0}
			<section>
				<h2 class="mb-3">Appareils partagés</h2>
				<div class="panel rows py-1">
					{#each shared as device (device.id)}
						{@const grants = deviceGrants(device)}
						{@const cut = grants.some((g) => g.target.kind === 'pause')}
						{@const blocked = data.device_blocked_today?.[device.id] ?? 0}
						<button class="row" onclick={() => { deviceId = device.id; deviceOpen = true; }}>
							<Shield size={46} opened={filterGrants(grants).length > 0} />
							<span class="min-w-0 flex-1">
								<span class="block font-semibold">{device.name}</span>
								<span class="muted block text-sm">
									{#if cut}Internet coupé{:else if filterGrants(grants).length > 0}{filterLine(blocked, grants, now)}{:else}{blocked} {filterLine(blocked, grants, now)}{/if}
								</span>
							</span>
							<ChevronRight size={20} class="text-muted shrink-0" />
						</button>
					{/each}
				</div>
			</section>
		{/if}

		{#if data.blocklists.categories === 0}
			<p class="muted text-center text-sm">
				Les listes de sites ne sont pas encore téléchargées.
				<a class="underline underline-offset-2" href="/reglages">Voir les réglages</a>
			</p>
		{/if}
	</div>

	{#if timeChild}
		<TimeSheet
			bind:open={timeOpen}
			name={timeChild.name}
			childId={timeChild.id}
			status={timeChild.status}
			line={line(timeChild)}
			grants={timeGrants(timeChild.grants)}
			now={data.now}
			ondone={load}
		/>
	{/if}
	{#if filterChild}
		<FilterSheet
			bind:open={filterOpen}
			name={filterChild.name}
			subject={{ child_id: filterChild.id }}
			{services}
			blockedServices={filterChild.blocked_services}
			grants={filterGrants(filterChild.grants)}
			now={data.now}
			blockedToday={filterChild.blocked_today ?? 0}
			activityHref="/enfants/{filterChild.id}"
			ondone={load}
		/>
	{/if}
	{#if sheetDevice}
		<FilterSheet
			bind:open={deviceOpen}
			name={sheetDevice.name}
			subject={{ device_id: sheetDevice.id }}
			{services}
			blockedServices={sheetDevice.policy?.filter.blocked_services ?? []}
			grants={deviceGrants(sheetDevice)}
			now={data.now}
			blockedToday={data.device_blocked_today?.[sheetDevice.id] ?? 0}
			pausable
			ondone={load}
		/>
	{/if}

	<Sheet bind:open={durationOpen} title="Accorder à {durationFor?.child_name ?? ''}">
		{#if durationFor}
			<p class="muted -mt-2 mb-4">{durationFor.label}, pendant combien de temps ?</p>
			<Segmented
				bind:value={duration}
				label="Durée accordée"
				options={[
					{ value: 15, label: '15 min' },
					{ value: 30, label: '30 min' },
					{ value: 60, label: '1 h' },
					{ value: 120, label: '2 h' }
				]}
			/>
			<button class="btn-primary btn-block mt-4" onclick={() => approve(durationFor!, duration)}>Accorder {minutes(duration)}</button>
		{/if}
	</Sheet>

	<Sheet bind:open={addOpen} title="Ajouter un enfant">
		<form class="space-y-4" onsubmit={addChild}>
			<div>
				<label class="label" for="child-name">Prénom</label>
				<input id="child-name" class="field" bind:value={newName} required maxlength="40" autocomplete="off" />
			</div>
			<div>
				<label class="label" for="child-year">Année de naissance</label>
				<input id="child-year" type="number" inputmode="numeric" class="field" bind:value={newYear} min={thisYear - 25} max={thisYear} placeholder={String(thisYear - 9)} />
				<p class="hint">Sert uniquement à proposer des réglages adaptés à son âge.</p>
			</div>
			<button class="btn-primary btn-block">Continuer</button>
		</form>
	</Sheet>
{/if}
