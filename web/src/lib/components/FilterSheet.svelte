<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import { grantRemaining, minutes, targetLabel } from '#lib/format.ts';
	import { cleanSite, serviceForSite, siteCount } from '#lib/services.ts';
	import type { Grant, GrantTarget, Service } from '#lib/types.ts';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import LockOpen from '@lucide/svelte/icons/lock-open';
	import Segmented from './Segmented.svelte';
	import ServiceLogo from './ServiceLogo.svelte';
	import Sheet from './Sheet.svelte';

	// Actions de filtrage : ouvrir un service ou un site pour un moment,
	// refermer une ouverture, couper Internet sur un appareil partagé.
	let {
		open = $bindable(false),
		name,
		subject,
		services,
		blockedServices,
		grants,
		now,
		blockedToday = 0,
		pausable = false,
		activityHref = null,
		ondone
	}: {
		open: boolean;
		name: string;
		subject: { child_id: string } | { device_id: string };
		services: Service[];
		blockedServices: string[];
		/** Ouvertures en cours (et pause, pour un appareil partagé). */
		grants: Grant[];
		now: number;
		blockedToday?: number;
		/** Appareil partagé : propose de couper Internet. */
		pausable?: boolean;
		activityHref?: string | null;
		ondone: () => void;
	} = $props();

	let duration = $state(30);
	let site = $state('');
	let busy = $state(false);

	const pause = $derived(grants.find((g) => g.target.kind === 'pause'));
	const openings = $derived(grants.filter((g) => g.target.kind !== 'pause'));
	const openIds = $derived(openings.map((g) => (g.target.kind === 'service' ? g.target.service : '')));
	const closed = $derived(services.filter((s) => blockedServices.includes(s.id) && !openIds.includes(s.id)));
	const badge = (target: GrantTarget) => (target.kind === 'service' ? services.find((s) => s.id === target.service) : undefined);
	// Un site qui fait partie d'un service ouvre le service entier.
	const whole = $derived(serviceForSite(services, site));

	function openSite() {
		const domain = cleanSite(site);
		if (!domain) return;
		if (whole) grant({ kind: 'service', service: whole.id }, `${whole.label} ouvert pour ${name} pendant ${minutes(duration)}`);
		else grant({ kind: 'domain', domain }, `${domain} ouvert pendant ${minutes(duration)}`);
	}

	async function grant(target: GrantTarget, message: string) {
		busy = true;
		const done = await attempt(() => api.post('/grants', { ...subject, target, minutes: duration }), message);
		busy = false;
		if (done) {
			site = '';
			open = false;
			ondone();
		}
	}

	async function revoke(g: Grant, message: string) {
		busy = true;
		const done = await attempt(() => api.del(`/grants/${g.id}`), message);
		busy = false;
		if (done) ondone();
	}
</script>

<Sheet bind:open title="Filtre de {name}">
	<div class="space-y-6">
		<p class="muted -mt-2">
			{#if blockedToday > 0}
				{blockedToday} {blockedToday > 1 ? 'tentatives bloquées' : 'tentative bloquée'} aujourd'hui.
			{:else}
				Rien n'a été bloqué aujourd'hui.
			{/if}
		</p>

		{#if openings.length > 0}
			<section>
				<h3 class="mb-1">Ouvert en ce moment</h3>
				<div class="rows">
					{#each openings as g (g.id)}
						{@const service = badge(g.target)}
						<div class="row">
							{#if service}<ServiceLogo label={service.label} logo={service.logo} size={38} />{/if}
							<span class="min-w-0 flex-1">
								<span class="font-semibold">{targetLabel(g.target, services)}</span>
								<span class="muted block text-sm">{grantRemaining(g, now)}</span>
							</span>
							<button class="btn-quiet btn-sm" disabled={busy} onclick={() => revoke(g, 'Refermé')}>Refermer</button>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<section>
			<h3 class="mb-2">Ouvrir pour un moment</h3>
			<Segmented
				bind:value={duration}
				label="Durée de l'ouverture"
				options={[
					{ value: 15, label: '15 min' },
					{ value: 30, label: '30 min' },
					{ value: 60, label: '1 h' },
					{ value: 120, label: '2 h' }
				]}
			/>
			{#if closed.length > 0}
				<div class="rows mt-2">
					{#each closed as service (service.id)}
						<button
							class="row"
							disabled={busy}
							onclick={() => grant({ kind: 'service', service: service.id }, `${service.label} ouvert pour ${name} pendant ${minutes(duration)}`)}
						>
							<ServiceLogo label={service.label} logo={service.logo} size={38} />
							<span class="min-w-0 flex-1 font-semibold">{service.label}</span>
							<span class="muted flex items-center gap-1 text-sm"><LockOpen size={17} /> {minutes(duration)}</span>
						</button>
					{/each}
				</div>
			{/if}
			<form
				class="mt-3"
				onsubmit={(e) => {
					e.preventDefault();
					openSite();
				}}
			>
				<label class="label" for="open-site">Un site précis</label>
				<div class="flex gap-2">
					<input id="open-site" class="field" placeholder="lumni.fr" bind:value={site} autocomplete="off" autocapitalize="off" inputmode="url" />
					<button class="btn-primary shrink-0" disabled={busy || !site.trim()}>Ouvrir</button>
				</div>
				{#if whole}
					<p class="bg-sun-soft mt-2 flex items-center gap-3 rounded-2xl px-3 py-2.5 text-sm">
						<ServiceLogo label={whole.label} logo={whole.logo} size={34} />
						<span>
							Ce site fait partie de <strong>{whole.label}</strong> : tout le service s'ouvrira, soit {siteCount(whole.domains)}.
						</span>
					</p>
				{/if}
			</form>
		</section>

		{#if pausable}
			<section>
				<h3 class="mb-2">Couper Internet</h3>
				{#if pause}
					<p class="mb-2">Coupé, {grantRemaining(pause, now)}.</p>
					<button class="btn-primary btn-block" disabled={busy} onclick={() => revoke(pause, `Internet rétabli sur ${name}`)}>Rétablir maintenant</button>
				{:else}
					<button class="btn-quiet btn-block" disabled={busy} onclick={() => grant({ kind: 'pause' }, `Internet coupé sur ${name} pendant ${minutes(duration)}`)}>
						Couper pendant {minutes(duration)}
					</button>
				{/if}
			</section>
		{/if}

		{#if activityHref}
			<a class="row justify-between border-t border-line" href={activityHref}>
				<span class="font-semibold">Voir ce qui a été bloqué</span>
				<ChevronRight size={20} class="text-muted" />
			</a>
		{/if}
	</div>
</Sheet>
