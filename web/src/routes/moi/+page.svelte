<script lang="ts">
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import About from '#lib/components/About.svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import Dial from '#lib/components/Dial.svelte';
	import Logo from '#lib/components/Logo.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import ServiceLogo from '#lib/components/ServiceLogo.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import Shield from '#lib/components/Shield.svelte';
	import WeekChart from '#lib/components/WeekChart.svelte';
	import { filterGrants, grantRemaining, minutes, nowMinutes, opening, seconds, targetLabel, timeLine } from '#lib/format.ts';
	import { cleanSite } from '#lib/services.ts';
	import type { ChildSpace, GrantTarget } from '#lib/types.ts';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Clock from '@lucide/svelte/icons/clock';
	import Globe from '@lucide/svelte/icons/globe';
	import LockOpen from '@lucide/svelte/icons/lock-open';
	import Moon from '@lucide/svelte/icons/moon';
	import Pause from '@lucide/svelte/icons/pause';
	import { onMount } from 'svelte';

	// Espace de l'enfant : son temps, son filtre, ses demandes, et ce que ses parents voient.
	const preview = $derived(page.url.searchParams.get('apercu'));
	const expired = $derived(page.url.searchParams.get('lien') === 'expire');

	let space = $state<ChildSpace | null>(null);
	let signedOut = $state(false);
	let minute = $state(nowMinutes());
	let width = $state(360);

	let timeOpen = $state(false);
	let siteOpen = $state(false);
	let askMinutes = $state(30);
	let message = $state('');
	// Ce que l'enfant veut ouvrir : un service proposé, ou un autre site.
	let choice = $state<string | null>(null);
	let site = $state('');
	let busy = $state(false);

	async function load() {
		try {
			space = await api.get<ChildSpace>(preview ? `/children/${preview}/space` : '/child/me');
			minute = nowMinutes();
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

	async function send(target: GrantTarget, duration?: number) {
		busy = true;
		const done = await attempt(
			() => api.post('/child/requests', { target, minutes: duration, message: message || undefined }),
			'Demande envoyée à tes parents'
		);
		busy = false;
		if (done) {
			timeOpen = false;
			siteOpen = false;
			choice = null;
			site = '';
			message = '';
			load();
		}
	}

	const status = $derived(space?.status);
	// Le jour quand l'écran est ouvert, la nuit quand il est fermé.
	const phase = $derived(status ? (status.open ? 'jour' : 'nuit') : undefined);
	const dial = $derived(Math.round(Math.min(310, Math.max(220, width - 56))));
	const remaining = $derived.by(() => {
		if (!space || !status?.quota_today_minutes) return null;
		return Math.max(0, 1 - space.used_today_seconds / 60 / status.quota_today_minutes);
	});
	const line = $derived(space && status ? timeLine(status, space.today_ranges, space.next_opening, minute) : '');
	const opened = $derived(space ? filterGrants(space.grants) : []);
	const blocked = $derived(space?.blocked_today ?? 0);

	const offered = $derived(space?.services.filter((s) => space?.requestable_services.includes(s.id)) ?? []);
	const service = (target: GrantTarget) => (target.kind === 'service' ? space?.services.find((s) => s.id === target.service) : undefined);
	// Sans service proposé, la feuille ne demande que le nom du site.
	const typing = $derived(choice === 'site' || offered.length === 0);
	const wanted = $derived.by((): { target: GrantTarget; label: string } | null => {
		const picked = offered.find((s) => s.id === choice);
		if (picked) return { target: { kind: 'service', service: picked.id }, label: picked.label };
		const domain = cleanSite(site);
		return typing && domain.includes('.') ? { target: { kind: 'domain', domain }, label: domain } : null;
	});

	const CLOSED: Record<string, string> = {
		paused: 'Tes parents ont mis l’écran en pause.',
		outside_schedule: 'Ce n’est pas l’heure de l’écran.',
		daily_quota: 'Tu as utilisé tout ton temps d’aujourd’hui.',
		weekly_quota: 'Tu as utilisé tout ton temps de la semaine.'
	};
	const STATUS = { pending: 'En attente', approved: 'Accordé', denied: 'Refusé' } as const;
	const STATUS_TAG = { pending: 'tag-sun', approved: 'tag-mint', denied: 'tag-quiet' } as const;
</script>

<div class="bg-bg text-ink min-h-dvh transition-colors duration-700" data-phase={phase}>
	<div class="mx-auto max-w-md px-4 pt-5 pb-12" bind:clientWidth={width}>
		{#if signedOut}
			<div class="flex min-h-[80dvh] flex-col justify-center">
				<div class="mb-6"><Logo size={52} /></div>
				<h1 class="text-[2.2rem]">{expired ? 'Ce lien a expiré.' : 'Ton espace Cotutelle'}</h1>
				<p class="muted mt-3 text-lg">
					Pour voir ton temps d'écran et faire une demande, ouvre « Mon temps d'écran » depuis le menu de ton
					ordinateur.
				</p>
			</div>
		{:else if space && status}
			{#if preview}
				<p class="bg-sun text-night mb-4 rounded-2xl px-4 py-3 text-center text-sm font-semibold">
					Aperçu parent : voici exactement ce que {space.child.name} voit.
				</p>
			{/if}

			<header class="flex items-center gap-3">
				<Avatar name={space.child.name} color={space.child.color} size={48} />
				<div>
					<p class="muted text-sm leading-tight">Bonjour</p>
					<p class="display text-2xl">{space.child.name}</p>
				</div>
			</header>

			<!-- Le cadran : la journée, les plages permises, le temps restant. -->
			<section class="mt-6 flex flex-col items-center text-center">
				<Dial size={dial} ranges={space.today_ranges} now={minute} {remaining} open={status.open} low={status.open && (status.remaining_minutes ?? 99) <= 5} labels>
					{#if status.open}
						{#if status.remaining_minutes === null}
							<span class="display text-5xl">Libre</span>
						{:else}
							<span class="display text-[3rem]">{minutes(status.remaining_minutes)}</span>
							<span class="muted mt-1">{status.remaining_minutes > 1 ? 'restantes' : 'restante'}</span>
						{/if}
					{:else if status.reason === 'paused'}
						<Pause size={56} />
					{:else}
						<Moon size={56} />
					{/if}
				</Dial>

				<h1 class="mt-5 text-[1.9rem]">
					{#if status.open}{line}{:else}{CLOSED[status.reason ?? 'daily_quota']}{/if}
				</h1>
				<p class="muted mt-1.5">
					{#if !status.open && status.reason === 'outside_schedule' && opening(space.next_opening)}
						Tu pourras reprendre {opening(space.next_opening)}.
					{:else if status.quota_today_minutes}
						Tu as utilisé {seconds(space.used_today_seconds)} sur {minutes(status.quota_today_minutes)} aujourd'hui.
					{:else}
						Tu as utilisé {seconds(space.used_today_seconds)} aujourd'hui.
					{/if}
				</p>
			</section>

			<!-- Le bouclier : ce que le filtre fait pour lui. -->
			<section class="panel mt-7 flex items-center gap-4">
				<Shield size={64} opened={opened.length > 0}>
					<span class="display text-lg">{blocked}</span>
				</Shield>
				<div class="min-w-0">
					<p class="font-semibold">Ton filtre est actif</p>
					<p class="muted text-sm">
						{#if blocked === 0}Rien n'a été bloqué aujourd'hui.{:else}{blocked} {blocked > 1 ? 'sites bloqués' : 'site bloqué'} aujourd'hui.{/if}
					</p>
				</div>
			</section>

			{#if space.grants.length > 0}
				<section class="panel mt-3">
					<h3>Tes parents t'ont accordé</h3>
					<div class="rows mt-1">
						{#each space.grants.filter((g) => g.target.kind !== 'pause') as grant (grant.id)}
							{@const badge = service(grant.target)}
							<div class="row min-h-12 py-1.5">
								{#if badge}<ServiceLogo label={badge.label} logo={badge.logo} size={34} />{/if}
								<span class="min-w-0 flex-1 font-semibold">{targetLabel(grant.target, space.services)}</span>
								<span class="muted text-sm">{grantRemaining(grant, space.now)}</span>
							</div>
						{/each}
					</div>
				</section>
			{/if}

			{#if space.allow_requests}
				<section class="mt-7">
					<h2 class="mb-3">Demander à tes parents</h2>
					<div class="grid gap-2">
						<button class="btn-primary min-h-16 justify-start px-5 text-lg" disabled={!!preview} onclick={() => (timeOpen = true)}>
							<Clock size={24} /> Plus de temps
						</button>
						<button class="btn-quiet bg-surface min-h-16 justify-start px-5 text-lg" disabled={!!preview} onclick={() => (siteOpen = true)}>
							<LockOpen size={24} /> Ouvrir un site ou une appli
						</button>
					</div>

					{#if space.requests.length > 0}
						<div class="rows panel mt-3 py-1">
							{#each space.requests as request (request.id)}
								<div class="row min-h-12 justify-between">
									<span class="min-w-0 truncate">
										{request.label}{#if request.minutes}, {minutes(request.minutes)}{/if}
									</span>
									<span class={STATUS_TAG[request.status]}>{STATUS[request.status]}</span>
								</div>
							{/each}
						</div>
					{/if}
				</section>
			{/if}

			<details class="panel group mt-7">
				<summary class="flex list-none items-center justify-between gap-3 font-semibold [&::-webkit-details-marker]:hidden">
					Ce que tes parents voient
					<ChevronDown size={20} class="text-muted transition-transform group-open:rotate-180" />
				</summary>
				<div class="mt-4 space-y-6">
					<p class="muted">
						Ils voient ton temps d'écran et le nom des sites. Rien d'autre : ni les pages, ni tes messages, ni tes
						recherches. Tu vois ici la même chose qu'eux.
					</p>
					<div>
						<h3 class="mb-2">Ton temps d'écran cette semaine</h3>
						<WeekChart days={space.activity.days} quota={status.quota_today_minutes} />
					</div>
					{#if space.activity.top_domains.length > 0}
						<div>
							<h3 class="mb-2">Les sites que tu utilises le plus</h3>
							<p class="leading-relaxed">{space.activity.top_domains.slice(0, 10).map((d) => d.domain).join(', ')}</p>
						</div>
					{/if}
					{#if space.activity.blocked.length > 0}
						<div>
							<h3 class="mb-2">Les sites qui ont été bloqués</h3>
							<p class="leading-relaxed">{space.activity.blocked.slice(0, 10).map((d) => d.domain).join(', ')}</p>
						</div>
					{/if}
				</div>
			</details>

			<About />
		{/if}
	</div>

	<Sheet bind:open={timeOpen} title="Demander plus de temps">
		<div class="space-y-4">
			<div>
				<label class="label" for="time-message">Un mot pour tes parents, si tu veux</label>
				<input id="time-message" class="field" bind:value={message} maxlength="200" placeholder="Je finis mon exposé" />
			</div>
			<div class="grid grid-cols-3 gap-2">
				{#each [15, 30, 60] as m (m)}
					<button class="btn-sun display min-h-16 text-xl" disabled={busy} onclick={() => send({ kind: 'extra_minutes', minutes: m })}>
						+{minutes(m)}
					</button>
				{/each}
			</div>
		</div>
	</Sheet>

	<Sheet bind:open={siteOpen} title="Demander une ouverture">
		{#if space}
			<form
				class="space-y-5"
				onsubmit={(e) => {
					e.preventDefault();
					if (wanted) send(wanted.target, askMinutes);
				}}
			>
				{#if offered.length > 0}
					<div class="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Ce que tu veux ouvrir">
						{#each offered as item (item.id)}
							<button type="button" role="radio" aria-checked={choice === item.id} class="choix" onclick={() => (choice = item.id)}>
								<ServiceLogo label={item.label} logo={item.logo} size={54} />
								<span>{item.label}</span>
							</button>
						{/each}
						<button type="button" role="radio" aria-checked={choice === 'site'} class="choix" onclick={() => (choice = 'site')}>
							<span class="border-muted text-muted flex h-[54px] w-[54px] items-center justify-center rounded-[15px] border-2 border-dashed">
								<Globe size={26} />
							</span>
							<span>Un autre site</span>
						</button>
					</div>
				{/if}
				{#if typing}
					<div>
						<label class="label" for="site">Le nom du site</label>
						<input id="site" class="field" placeholder="exemple.fr" bind:value={site} autocomplete="off" autocapitalize="off" inputmode="url" />
					</div>
				{/if}
				<div>
					<p class="label">Pendant combien de temps ?</p>
					<Segmented
						bind:value={askMinutes}
						label="Durée demandée"
						options={[
							{ value: 15, label: '15 min' },
							{ value: 30, label: '30 min' },
							{ value: 60, label: '1 h' },
							{ value: 120, label: '2 h' }
						]}
					/>
				</div>
				<div>
					<label class="label" for="site-message">Un mot pour tes parents, si tu veux</label>
					<input id="site-message" class="field" bind:value={message} maxlength="200" placeholder="Pour parler avec Tom" />
				</div>
				<button class="btn-primary btn-block min-h-14 py-2" disabled={busy || !wanted}>
					{#if wanted}Demander {wanted.label} pendant {minutes(askMinutes)}{:else}Demander{/if}
				</button>
			</form>
		{/if}
	</Sheet>
</div>

<style>
	/* Un service à demander : son logo en grand, comme sur un écran d'accueil. */
	.choix {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.5rem;
		min-height: 6.75rem;
		padding: 0.85rem 0.3rem 0.7rem;
		border-radius: 22px;
		background: var(--bg);
		font-size: 0.9rem;
		font-weight: 650;
		line-height: 1.15;
		text-align: center;
		overflow-wrap: anywhere;
		transition:
			transform 0.12s ease,
			background-color 0.15s ease;
	}
	.choix:active {
		transform: scale(0.97);
	}
	.choix[aria-checked='true'] {
		background: var(--sun-soft);
		box-shadow: inset 0 0 0 3px var(--sun);
	}
</style>
