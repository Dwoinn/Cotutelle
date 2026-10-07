<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import { minutes } from '#lib/format.ts';
	import type { GrantTarget, Service } from '#lib/types.ts';
	import Modal from './Modal.svelte';

	// Fenêtre « Autoriser » : accorde une exception à un enfant ou à un appareil.
	let {
		open = $bindable(false),
		name,
		subject,
		services,
		blockedServices,
		withTime = true,
		ondone
	}: {
		open: boolean;
		name: string;
		subject: { child_id: string } | { device_id: string };
		services: Service[];
		blockedServices: string[];
		withTime?: boolean;
		ondone: () => void;
	} = $props();

	const DURATIONS = [15, 30, 60, 120];
	let duration = $state(30);
	let domain = $state('');
	let busy = $state(false);

	const unlockable = $derived(services.filter((s) => blockedServices.includes(s.id)));

	async function grant(target: GrantTarget, label: string, withDuration = true) {
		busy = true;
		const body = { ...subject, target, minutes: withDuration ? duration : undefined };
		const done = await attempt(() => api.post('/grants', body), `${label} : accordé à ${name}`);
		busy = false;
		if (done) {
			open = false;
			domain = '';
			ondone();
		}
	}
</script>

<Modal bind:open title="Autoriser pour {name}">
	<div class="space-y-5">
		<div>
			<p class="label">Pendant combien de temps ?</p>
			<div class="flex flex-wrap gap-2">
				{#each DURATIONS as d (d)}
					<button class={duration === d ? 'btn-primary btn-sm' : 'btn-soft btn-sm'} onclick={() => (duration = d)}>
						{minutes(d)}
					</button>
				{/each}
			</div>
		</div>

		{#if unlockable.length > 0}
			<div>
				<p class="label">Un service bloqué</p>
				<div class="grid grid-cols-2 gap-2">
					{#each unlockable as service (service.id)}
						<button
							class="btn-ghost justify-start border border-slate-200 dark:border-slate-700"
							disabled={busy}
							onclick={() => grant({ kind: 'service', service: service.id }, service.label)}
						>
							<span>{service.icon}</span>
							{service.label}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		<form
			onsubmit={(e) => {
				e.preventDefault();
				if (domain.trim()) grant({ kind: 'domain', domain }, domain.trim());
			}}
		>
			<label class="label" for="allow-domain">Un site précis</label>
			<div class="flex gap-2">
				<input id="allow-domain" class="input" placeholder="exemple.fr" bind:value={domain} autocomplete="off" />
				<button class="btn-primary" disabled={busy || !domain.trim()}>Autoriser</button>
			</div>
		</form>

		{#if withTime}
			<div>
				<p class="label">En dehors des horaires habituels</p>
				<button class="btn-soft" disabled={busy} onclick={() => grant({ kind: 'ignore_schedule' }, 'Hors horaires')}>
					🌙 Ouvrir l'accès pendant {minutes(duration)}
				</button>
			</div>
		{/if}
	</div>
</Modal>
