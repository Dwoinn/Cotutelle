<script lang="ts">
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import favicon from '#lib/assets/favicon.svg';
	import Modal from '#lib/components/Modal.svelte';
	import Ring from '#lib/components/Ring.svelte';
	import WeekChart from '#lib/components/WeekChart.svelte';
	import { minutes, opening, range, seconds, targetIcon, targetLabel, until } from '#lib/format.ts';
	import type { ChildSpace, GrantTarget } from '#lib/types.ts';
	import { onMount } from 'svelte';

	// Espace de l'enfant : son temps, ses demandes, et ce que ses parents voient.
	const preview = $derived(page.url.searchParams.get('apercu'));
	const expired = $derived(page.url.searchParams.get('lien') === 'expire');

	let space = $state<ChildSpace | null>(null);
	let signedOut = $state(false);

	let askOpen = $state(false);
	let ask = $state<{ title: string; target: GrantTarget; timed: boolean } | null>(null);
	let askMinutes = $state(30);
	let askMessage = $state('');
	let site = $state('');
	let busy = $state(false);

	async function load() {
		try {
			space = await api.get<ChildSpace>(preview ? `/children/${preview}/space` : '/child/me');
			signedOut = false;
		} catch (error) {
			if (error instanceof ApiError && error.status === 401) signedOut = true;
		}
	}

	onMount(() => {
		load();
		const timer = setInterval(load, 15_000);
		return () => clearInterval(timer);
	});

	function openAsk(title: string, target: GrantTarget, timed = true) {
		ask = { title, target, timed };
		askMinutes = 30;
		askMessage = '';
		askOpen = true;
	}

	async function send() {
		if (!ask) return;
		busy = true;
		const body = { target: ask.target, minutes: ask.timed ? askMinutes : undefined, message: askMessage || undefined };
		const done = await attempt(() => api.post('/child/requests', body), 'Demande envoyée à tes parents');
		busy = false;
		if (done) {
			askOpen = false;
			site = '';
			load();
		}
	}

	const status = $derived(space?.status);
	const color = $derived(space?.child.color ?? '#6d5dfc');

	const ring = $derived.by(() => {
		if (!space || !status) return 0;
		if (!status.open) return 0;
		if (!status.quota_today_minutes) return 1;
		return Math.max(0, 1 - space.used_today_seconds / 60 / status.quota_today_minutes);
	});

	const CLOSED: Record<string, { icon: string; title: string; text: string }> = {
		paused: { icon: '⏸️', title: 'Écran en pause', text: 'Tes parents ont mis l’écran en pause pour le moment.' },
		outside_schedule: { icon: '🌙', title: 'Ce n’est pas l’heure', text: 'Tu es en dehors de tes horaires d’écran.' },
		daily_quota: { icon: '✅', title: 'C’est fini pour aujourd’hui', text: 'Tu as utilisé tout ton temps d’écran du jour.' },
		weekly_quota: { icon: '✅', title: 'C’est fini pour cette semaine', text: 'Tu as utilisé tout ton temps d’écran de la semaine.' }
	};

	const STATUS_LABEL = { pending: '⏳ En attente', approved: '✅ Accordé', denied: '❌ Refusé' } as const;
	const pending = $derived(space?.requests.filter((r) => r.status === 'pending').length ?? 0);
</script>

<div class="mx-auto min-h-screen max-w-xl px-4 pt-6 pb-12">
	{#if signedOut}
		<div class="card mt-16 text-center">
			<img src={favicon} alt="" class="mx-auto mb-3 h-14 w-14" />
			<h1>{expired ? 'Ce lien a expiré' : 'Ton espace Cotutelle'}</h1>
			<p class="muted mt-2">
				Pour voir ton temps d'écran et faire une demande, ouvre « Mon temps d'écran » depuis le menu de ton
				ordinateur.
			</p>
		</div>
	{:else if !space || !status}
		<p class="muted mt-16 text-center">Chargement…</p>
	{:else}
		{#if preview}
			<p class="pill-warn mb-4 w-full justify-center rounded-2xl py-2 text-sm">
				Aperçu parent : voici exactement ce que {space.child.name} voit.
			</p>
		{/if}

		<header class="mb-6 flex items-center gap-3">
			<span class="flex h-14 w-14 items-center justify-center rounded-3xl text-3xl" style:background="{color}22">
				{space.child.emoji}
			</span>
			<div>
				<p class="muted">Bonjour</p>
				<h1>{space.child.name}</h1>
			</div>
		</header>

		<section class="card flex flex-col items-center gap-4 py-8 text-center">
			{#if status.open}
				<Ring value={ring} size={220} stroke={18} {color}>
					{#if status.remaining_minutes === null}
						<span class="text-5xl font-bold">∞</span>
						<span class="muted">pas de limite</span>
					{:else}
						<span class="text-4xl font-bold tabular-nums">{minutes(status.remaining_minutes)}</span>
						<span class="muted">restantes</span>
					{/if}
				</Ring>
				<p class="muted">
					Tu as utilisé <strong class="text-slate-700 dark:text-slate-200">{seconds(space.used_today_seconds)}</strong>
					aujourd'hui{#if status.quota_today_minutes}&nbsp;sur {minutes(status.quota_today_minutes)}{/if}.
				</p>
			{:else}
				{@const info = CLOSED[status.reason ?? 'daily_quota']}
				<p class="text-6xl">{info.icon}</p>
				<h2 class="text-2xl">{info.title}</h2>
				<p class="muted max-w-xs">{info.text}</p>
				{#if status.reason === 'outside_schedule' && opening(space.next_opening)}
					<p class="pill-ok text-sm">Reprise {opening(space.next_opening)}</p>
				{/if}
			{/if}

			{#if space.today_ranges.length > 0}
				<div class="flex flex-wrap justify-center gap-2">
					<span class="muted text-xs">Tes horaires aujourd'hui :</span>
					{#each space.today_ranges as r, i (i)}
						<span class="chip">{range(r)}</span>
					{/each}
				</div>
			{/if}
		</section>

		{#if space.grants.length > 0}
			<section class="card mt-4">
				<h2 class="mb-2">Accordé par tes parents</h2>
				<ul class="space-y-2">
					{#each space.grants as grant (grant.id)}
						<li class="flex items-center justify-between gap-3 text-sm">
							<span>{targetIcon(grant.target, [])} {targetLabel(grant.target, space.blocked_services.map((s) => ({ ...s, domains: [] })))}</span>
							<span class="muted">
								{grant.target.kind === 'extra_minutes' ? "aujourd'hui" : `encore ${until(grant.expires_at, space.now)}`}
							</span>
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		{#if space.allow_requests}
			<section class="card mt-4 space-y-4">
				<div>
					<h2>Demander à tes parents</h2>
					<p class="muted">Ils reçoivent ta demande et décident.</p>
				</div>

				<div>
					<p class="label">Plus de temps</p>
					<div class="flex flex-wrap gap-2">
						{#each [15, 30, 60] as m (m)}
							<button
								class="btn-soft"
								disabled={!!preview}
								onclick={() => openAsk(`Demander ${minutes(m)} de plus`, { kind: 'extra_minutes', minutes: m }, false)}
							>
								⏱️ +{minutes(m)}
							</button>
						{/each}
					</div>
				</div>

				{#if space.blocked_services.length > 0}
					<div>
						<p class="label">Ouvrir un service</p>
						<div class="grid grid-cols-2 gap-2">
							{#each space.blocked_services as service (service.id)}
								<button
									class="btn-ghost justify-start border border-slate-200 dark:border-slate-700"
									disabled={!!preview}
									onclick={() => openAsk(`Demander ${service.label}`, { kind: 'service', service: service.id })}
								>
									<span>{service.icon}</span>{service.label}
								</button>
							{/each}
						</div>
					</div>
				{/if}

				<form
					onsubmit={(e) => {
						e.preventDefault();
						if (site.trim()) openAsk(`Demander ${site.trim()}`, { kind: 'domain', domain: site.trim() });
					}}
				>
					<label class="label" for="site">Un site bloqué</label>
					<div class="flex gap-2">
						<input id="site" class="input" placeholder="exemple.fr" bind:value={site} autocomplete="off" />
						<button class="btn-soft" disabled={!!preview || !site.trim()}>Demander</button>
					</div>
				</form>

				{#if space.requests.length > 0}
					<div>
						<p class="label">Tes demandes{pending > 0 ? ` (${pending} en attente)` : ''}</p>
						<ul class="space-y-1.5">
							{#each space.requests as request (request.id)}
								<li class="flex items-center justify-between gap-3 text-sm">
									<span class="truncate">
										{request.label}{#if request.minutes}&nbsp;· {minutes(request.minutes)}{/if}
									</span>
									<span class="chip shrink-0">{STATUS_LABEL[request.status]}</span>
								</li>
							{/each}
						</ul>
					</div>
				{/if}
			</section>
		{/if}

		<details class="card mt-4">
			<summary class="font-semibold">👀 Ce que tes parents voient</summary>
			<div class="mt-4 space-y-5">
				<p class="muted">
					Tes parents voient ton temps d'écran et le nom des sites, rien de plus : ni les pages, ni tes messages,
					ni tes recherches. Tu vois ici exactement la même chose qu'eux.
				</p>
				<div>
					<p class="label">Ton temps d'écran cette semaine</p>
					<WeekChart days={space.activity.days} quota={status.quota_today_minutes} {color} />
				</div>
				{#if space.activity.top_domains.length > 0}
					<div>
						<p class="label">Les sites que tu utilises le plus</p>
						<div class="flex flex-wrap gap-2">
							{#each space.activity.top_domains.slice(0, 10) as d (d.domain)}
								<span class="chip">{d.domain}</span>
							{/each}
						</div>
					</div>
				{/if}
				{#if space.activity.blocked.length > 0}
					<div>
						<p class="label">Les sites qui ont été bloqués</p>
						<div class="flex flex-wrap gap-2">
							{#each space.activity.blocked.slice(0, 10) as d (d.domain)}
								<span class="chip">{d.domain} · {d.blocked}×</span>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		</details>
	{/if}
</div>

<Modal bind:open={askOpen} title={ask?.title ?? ''}>
	{#if ask}
		<div class="space-y-4">
			{#if ask.timed}
				<div>
					<p class="label">Pendant combien de temps ?</p>
					<div class="flex flex-wrap gap-2">
						{#each [15, 30, 60, 120] as m (m)}
							<button class={askMinutes === m ? 'btn-primary btn-sm' : 'btn-soft btn-sm'} onclick={() => (askMinutes = m)}>
								{minutes(m)}
							</button>
						{/each}
					</div>
				</div>
			{/if}
			<div>
				<label class="label" for="ask-message">Un mot pour tes parents (facultatif)</label>
				<input id="ask-message" class="input" bind:value={askMessage} maxlength="200" placeholder="C'est pour…" />
			</div>
			<button class="btn-primary w-full" disabled={busy} onclick={send}>Envoyer la demande</button>
		</div>
	{/if}
</Modal>
