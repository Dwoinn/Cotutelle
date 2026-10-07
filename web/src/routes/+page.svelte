<script lang="ts">
	import { goto } from '$app/navigation';
	import { ApiError, api } from '#lib/api.ts';
	import { app, attempt, catalog } from '#lib/app.svelte.ts';
	import AllowModal from '#lib/components/AllowModal.svelte';
	import GrantChips from '#lib/components/GrantChips.svelte';
	import Modal from '#lib/components/Modal.svelte';
	import Ring from '#lib/components/Ring.svelte';
	import { CLOSED_LABEL, ago, minutes, opening, seconds } from '#lib/format.ts';
	import type { Child, ChildRequest, ChildSummary, Dashboard, Device, Grant, Service } from '#lib/types.ts';
	import { onMount } from 'svelte';

	let data = $state<Dashboard | null>(null);
	let services = $state<Service[]>([]);
	let failed = $state(false);

	// Fenêtres ouvertes
	let allowFor = $state<null | { name: string; subject: { child_id: string } | { device_id: string }; blocked: string[]; withTime: boolean }>(null);
	let allowOpen = $state(false);
	let addOpen = $state(false);
	let newName = $state('');
	let newYear = $state<number | null>(null);

	async function load() {
		try {
			data = await api.get<Dashboard>('/dashboard');
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
	const thisYear = new Date().getFullYear();

	async function grant(body: unknown, message: string) {
		if (await attempt(() => api.post('/grants', body), message)) load();
	}

	async function revoke(g: Grant) {
		if (await attempt(() => api.del(`/grants/${g.id}`), 'Retiré')) load();
	}

	const bonus = (child: ChildSummary, m: number) =>
		grant({ child_id: child.id, target: { kind: 'extra_minutes', minutes: m } }, `+${minutes(m)} pour ${child.name}`);

	const pause = (subject: object, name: string, m: number) =>
		grant({ ...subject, target: { kind: 'pause' }, minutes: m }, `${name} : en pause pendant ${minutes(m)}`);

	function openAllow(name: string, subject: { child_id: string } | { device_id: string }, blocked: string[], withTime: boolean) {
		allowFor = { name, subject, blocked, withTime };
		allowOpen = true;
	}

	async function approve(request: ChildRequest, m?: number) {
		const body = m ? { minutes: m } : {};
		if (await attempt(() => api.post(`/requests/${request.id}/approve`, body), `Accordé à ${request.child_name}`)) load();
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

	function ringValue(child: ChildSummary): number {
		const quota = child.status.quota_today_minutes;
		if (!quota) return child.status.open ? 1 : 0;
		return Math.max(0, 1 - child.used_today_seconds / 60 / quota);
	}

	function headline(child: ChildSummary): string {
		if (!child.status.open) return '0';
		const left = child.status.remaining_minutes;
		return left === null ? '∞' : minutes(left);
	}

	const paused = (grants: Grant[]) => grants.some((g) => g.target.kind === 'pause');
	const devicePolicyBlocked = (device: Device) => device.policy?.filter.blocked_services ?? [];
</script>

{#if failed && !data}
	<div class="card text-center">
		<p class="font-medium">Le serveur ne répond pas.</p>
		<button class="btn-soft mt-3" onclick={load}>Réessayer</button>
	</div>
{:else if !data}
	<p class="muted">Chargement…</p>
{:else}
	<div class="space-y-6">
		<!-- Demandes en attente : la première chose à traiter. -->
		{#each data.requests as request (request.id)}
			<div class="card border-brand-200 bg-brand-50 dark:border-brand-800 dark:bg-brand-900/20">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<p class="font-semibold">
							🙋 {request.child_name} demande {request.label}
							{#if request.minutes}<span class="font-normal">pendant {minutes(request.minutes)}</span>{/if}
						</p>
						{#if request.message}<p class="mt-0.5 text-sm italic">« {request.message} »</p>{/if}
						<p class="muted text-xs">{ago(request.created_at, data.now)}</p>
					</div>
					<div class="flex flex-wrap gap-2">
						{#if request.target?.kind !== 'extra_minutes'}
							{#each [15, 30, 60].filter((m) => m !== request.minutes) as m (m)}
								<button class="btn-ghost btn-sm" onclick={() => approve(request, m)}>{minutes(m)}</button>
							{/each}
						{/if}
						<button class="btn-primary btn-sm" onclick={() => approve(request)}>
							Accorder{request.minutes ? ` ${minutes(request.minutes)}` : ''}
						</button>
						<button class="btn-danger btn-sm" onclick={() => deny(request)}>Refuser</button>
					</div>
				</div>
			</div>
		{/each}

		{#each data.alerts as alert (alert.id)}
			<div class="card flex items-center justify-between gap-3 border-amber-200 bg-amber-50 py-3 dark:border-amber-900 dark:bg-amber-950/30">
				<p class="text-sm">
					⚠️ {alert.message}
					<span class="muted text-xs">· {ago(alert.created_at, data.now)}</span>
				</p>
				<button class="btn-ghost btn-sm" onclick={() => ack(alert.id)}>Vu</button>
			</div>
		{/each}

		<section>
			<div class="mb-3 flex items-center justify-between">
				<h1>Les enfants</h1>
				<button class="btn-soft" onclick={() => (addOpen = true)}>+ Ajouter</button>
			</div>

			{#if data.children.length === 0}
				<div class="card py-10 text-center">
					<p class="text-4xl">👋</p>
					<h2 class="mt-2">Commençons par ajouter un enfant</h2>
					<p class="muted mx-auto mt-1 max-w-md">
						Chaque enfant a ses propres filtres, horaires et temps d'écran. Une configuration adaptée à son âge
						est proposée, que vous pourrez ajuster.
					</p>
					<button class="btn-primary mt-4" onclick={() => (addOpen = true)}>Ajouter un enfant</button>
				</div>
			{:else}
				<div class="grid gap-4 md:grid-cols-2">
					{#each data.children as child (child.id)}
						{@const isPaused = paused(child.grants)}
						<article class="card space-y-4">
							<div class="flex items-center gap-4">
								<Ring value={ringValue(child)} size={104} color={child.status.open ? child.color : '#94a3b8'}>
									<span class="text-xl leading-none font-bold">{headline(child)}</span>
									<span class="muted text-[10px]">{child.status.open ? 'restantes' : 'restant'}</span>
								</Ring>
								<div class="min-w-0 flex-1">
									<a href="/enfants/{child.id}" class="flex items-center gap-2 text-lg font-bold hover:underline">
										<span class="text-2xl">{child.emoji}</span>
										<span class="truncate">{child.name}</span>
									</a>
									<p class="mt-1">
										{#if child.status.open}
											<span class="pill-ok">Accès ouvert</span>
										{:else if child.status.reason}
											<span class={child.status.reason === 'paused' ? 'pill-warn' : 'pill-off'}>
												{CLOSED_LABEL[child.status.reason]}
											</span>
										{/if}
									</p>
									<p class="muted mt-1.5 text-xs">
										{seconds(child.used_today_seconds)} aujourd'hui{#if child.status.quota_today_minutes}
											&nbsp;sur {minutes(child.status.quota_today_minutes)}{/if}
										{#if !child.status.open && child.status.reason === 'outside_schedule' && opening(child.next_opening)}
											<br />Reprise {opening(child.next_opening)}
										{/if}
									</p>
								</div>
							</div>

							{#if child.devices.length > 0}
								<div class="flex flex-wrap gap-2">
									{#each child.devices as device (device.id)}
										<span class="chip">
											<span
												class="h-2 w-2 rounded-full {device.active
													? 'bg-emerald-500'
													: device.online || device.kind === 'network'
														? 'bg-slate-400'
														: 'bg-rose-400'}"
											></span>
											{device.name}
											<span class="opacity-60">
												{device.active ? 'en cours' : device.kind === 'network' ? 'réseau' : device.online ? 'connecté' : 'hors ligne'}
											</span>
										</span>
									{/each}
								</div>
							{:else}
								<p class="pill-warn rounded-2xl px-3 py-2 text-xs">
									Aucun appareil rattaché. <a class="underline" href="/appareils">Ajouter un appareil</a>
								</p>
							{/if}

							<GrantChips grants={child.grants} {services} now={data.now} onrevoke={revoke} />

							<div class="flex flex-wrap gap-2 border-t border-slate-100 pt-3 dark:border-slate-800">
								{#each [15, 30, 60] as m (m)}
									<button class="btn-soft btn-sm" onclick={() => bonus(child, m)}>+{minutes(m)}</button>
								{/each}
								<button
									class="btn-soft btn-sm"
									onclick={() => openAllow(child.name, { child_id: child.id }, child.blocked_services, true)}
								>
									🔓 Autoriser…
								</button>
								{#if !isPaused}
									<button class="btn-ghost btn-sm ml-auto" onclick={() => pause({ child_id: child.id }, child.name, 60)}>
										⏸️ Pause 1 h
									</button>
								{/if}
							</div>
						</article>
					{/each}
				</div>
			{/if}
		</section>

		{#if shared.length > 0}
			<section>
				<h2 class="mb-3">Appareils partagés</h2>
				<div class="grid gap-4 md:grid-cols-2">
					{#each shared as device (device.id)}
						{@const grants = data.device_grants.filter((g) => g.device_id === device.id)}
						<article class="card space-y-3">
							<div class="flex items-center justify-between gap-3">
								<p class="flex items-center gap-2 font-semibold"><span class="text-2xl">📺</span>{device.name}</p>
								{#if paused(grants)}
									<span class="pill-warn">En pause</span>
								{:else}
									<span class="pill-ok">Filtré</span>
								{/if}
							</div>
							<GrantChips {grants} {services} now={data.now} onrevoke={revoke} />
							<div class="flex flex-wrap gap-2">
								<button
									class="btn-soft btn-sm"
									onclick={() => openAllow(device.name, { device_id: device.id }, devicePolicyBlocked(device), false)}
								>
									🔓 Autoriser…
								</button>
								{#if !paused(grants)}
									<button class="btn-ghost btn-sm" onclick={() => pause({ device_id: device.id }, device.name, 60)}>
										⏸️ Pause 1 h
									</button>
								{/if}
							</div>
						</article>
					{/each}
				</div>
			</section>
		{/if}

		{#if data.blocklists.categories === 0}
			<p class="muted text-center text-xs">
				Les listes de blocage ne sont pas encore téléchargées.
				<a class="underline" href="/reglages">Voir les réglages</a>
			</p>
		{/if}
	</div>

	{#if allowFor}
		<AllowModal
			bind:open={allowOpen}
			name={allowFor.name}
			subject={allowFor.subject}
			{services}
			blockedServices={allowFor.blocked}
			withTime={allowFor.withTime}
			ondone={load}
		/>
	{/if}

	<Modal bind:open={addOpen} title="Ajouter un enfant">
		<form class="space-y-4" onsubmit={addChild}>
			<div>
				<label class="label" for="child-name">Prénom</label>
				<input id="child-name" class="input" bind:value={newName} required maxlength="40" />
			</div>
			<div>
				<label class="label" for="child-year">Année de naissance</label>
				<input id="child-year" type="number" class="input" bind:value={newYear} min={thisYear - 25} max={thisYear} placeholder={String(thisYear - 9)} />
				<p class="muted mt-1 text-xs">Sert uniquement à proposer des réglages adaptés à son âge.</p>
			</div>
			<button class="btn-primary w-full">Continuer</button>
		</form>
	</Modal>
{/if}
