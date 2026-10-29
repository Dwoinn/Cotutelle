<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import { grantRemaining, minutes, targetLabel } from '#lib/format.ts';
	import { cleanSite, serviceForSite, siteCount } from '#lib/services.ts';
	import type { Grant, GrantTarget, Service } from '#lib/types.ts';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Globe from '@lucide/svelte/icons/globe';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import DurationPicker from './DurationPicker.svelte';
	import ServiceLogo from './ServiceLogo.svelte';
	import Sheet from './Sheet.svelte';

	// Actions de filtrage : ouvrir un service ou un site pour un moment,
	// refermer une ouverture, couper Internet sur un appareil partagé.
	// On choisit d'abord quoi, puis pour combien de temps.
	let {
		open = $bindable(false),
		name,
		subject,
		services,
		blockedServices,
		requestableServices = null,
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
		/** Enfant : seuls ces services sont proposés. `null` (appareil partagé) : tous les bloqués. */
		requestableServices?: string[] | null;
		/** Ouvertures en cours (et pause, pour un appareil partagé). */
		grants: Grant[];
		now: number;
		blockedToday?: number;
		/** Appareil partagé : propose de couper Internet. */
		pausable?: boolean;
		activityHref?: string | null;
		ondone: () => void;
	} = $props();

	let site = $state('');
	let busy = $state(false);
	/** Ce qui attend une durée ; `null` = la liste. */
	let pending = $state<GrantTarget | null>(null);

	const pause = $derived(grants.find((g) => g.target.kind === 'pause'));
	const openings = $derived(grants.filter((g) => g.target.kind !== 'pause'));
	const openIds = $derived(openings.map((g) => (g.target.kind === 'service' ? g.target.service : '')));
	const closed = $derived(
		services.filter(
			(s) => blockedServices.includes(s.id) && (requestableServices === null || requestableServices.includes(s.id)) && !openIds.includes(s.id)
		)
	);
	const badge = (target: GrantTarget) => (target.kind === 'service' ? services.find((s) => s.id === target.service) : undefined);
	// Un site qui fait partie d'un service ouvre le service entier.
	const whole = $derived(serviceForSite(services, site));
	const pendingService = $derived(pending ? badge(pending) : undefined);

	// La feuille rouverte repart de la liste, champ vide.
	$effect(() => {
		if (!open) {
			pending = null;
			site = '';
		}
	});

	function askSite() {
		const domain = cleanSite(site);
		if (!domain) return;
		pending = whole ? { kind: 'service', service: whole.id } : { kind: 'domain', domain };
	}

	function message(target: GrantTarget, duration: number): string {
		const span = minutes(duration);
		if (target.kind === 'pause') return `Internet coupé sur ${name} pendant ${span}`;
		if (target.kind === 'domain') return `${target.domain} ouvert pendant ${span}`;
		return `${targetLabel(target, services)} ouvert pour ${name} pendant ${span}`;
	}

	async function grant(target: GrantTarget, duration: number) {
		busy = true;
		const done = await attempt(() => api.post('/grants', { ...subject, target, minutes: duration }), message(target, duration));
		busy = false;
		if (done) {
			site = '';
			pending = null;
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
	{#if pending}
		{@const target = pending}
		<!-- Second temps : pour combien de temps ? -->
		<div class="space-y-5">
			<button class="btn-ghost btn-sm -ml-3" onclick={() => (pending = null)}><ArrowLeft size={18} /> Retour</button>
			<div class="flex items-center gap-3">
				{#if pendingService}
					<ServiceLogo label={pendingService.label} logo={pendingService.logo} size={50} />
				{:else if target.kind === 'pause'}
					<span class="bg-sunken flex h-[50px] w-[50px] shrink-0 items-center justify-center rounded-2xl"><WifiOff size={26} /></span>
				{:else}
					<span class="bg-sunken flex h-[50px] w-[50px] shrink-0 items-center justify-center rounded-2xl"><Globe size={26} /></span>
				{/if}
				<div class="min-w-0">
					<p class="display truncate text-[1.35rem] leading-tight">
						{#if target.kind === 'pause'}Couper Internet{:else}{targetLabel(target, services)}{/if}
					</p>
					<p class="muted text-sm">
						{#if target.kind === 'pause'}Sur {name}, pendant combien de temps ?{:else}Ouvrir pour {name}, pendant combien de temps ?{/if}
					</p>
				</div>
			</div>
			<DurationPicker {busy} onpick={(m) => grant(target, m)} />
		</div>
	{:else}
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
				{#if closed.length > 0}
					<div class="rows">
						{#each closed as service (service.id)}
							<button class="row" disabled={busy} onclick={() => (pending = { kind: 'service', service: service.id })}>
								<ServiceLogo label={service.label} logo={service.logo} size={38} />
								<span class="min-w-0 flex-1 font-semibold">{service.label}</span>
								<ChevronRight size={20} class="text-muted shrink-0" />
							</button>
						{/each}
					</div>
				{:else if requestableServices !== null}
					<p class="muted text-sm">Aucun service « sur demande » dans son filtre. Un site précis reste possible.</p>
				{/if}
				<form
					class="mt-3"
					onsubmit={(e) => {
						e.preventDefault();
						askSite();
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
						<button class="btn-quiet btn-block" disabled={busy} onclick={() => (pending = { kind: 'pause' })}>
							<WifiOff size={20} /> Couper pour un moment
						</button>
					{/if}
				</section>
			{/if}

			{#if activityHref}
				<a class="row border-line justify-between border-t" href={activityHref}>
					<span class="font-semibold">Voir ce qui a été bloqué</span>
					<ChevronRight size={20} class="text-muted" />
				</a>
			{/if}
		</div>
	{/if}
</Sheet>
