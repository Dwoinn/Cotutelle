<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt } from '#lib/app.svelte.ts';
	import { grantRemaining, minutes, targetLabel } from '#lib/format.ts';
	import type { AccessStatus, Grant, GrantTarget } from '#lib/types.ts';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';
	import Segmented from './Segmented.svelte';
	import Sheet from './Sheet.svelte';

	// Actions de temps pour un enfant : ajouter, mettre en pause, ouvrir hors horaires.
	let {
		open = $bindable(false),
		name,
		childId,
		status,
		line,
		grants,
		now,
		ondone
	}: {
		open: boolean;
		name: string;
		childId: string;
		status: AccessStatus;
		/** Phrase d'état affichée sous le titre. */
		line: string;
		/** Mesures de temps en cours pour cet enfant. */
		grants: Grant[];
		now: number;
		ondone: () => void;
	} = $props();

	let pauseMinutes = $state(60);
	let busy = $state(false);

	const pause = $derived(grants.find((g) => g.target.kind === 'pause'));
	const others = $derived(grants.filter((g) => g.target.kind !== 'pause'));

	async function grant(target: GrantTarget, duration: number | undefined, message: string) {
		busy = true;
		const done = await attempt(() => api.post('/grants', { child_id: childId, target, minutes: duration }), message);
		busy = false;
		if (done) {
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

<Sheet bind:open title="Temps de {name}">
	<div class="space-y-6">
		<p class="muted -mt-2">
			{#if status.open && status.remaining_minutes !== null}
				Il reste {minutes(status.remaining_minutes)}. {line}.
			{:else}
				{line}.
			{/if}
		</p>

		{#if pause}
			<div class="bg-sun-soft rounded-3xl p-4">
				<p class="font-semibold">En pause, {grantRemaining(pause, now)}</p>
				<button class="btn-primary btn-block mt-3" disabled={busy} onclick={() => revoke(pause, `${name} peut reprendre`)}>
					<Play size={20} /> Reprendre maintenant
				</button>
			</div>
		{/if}

		<section>
			<h3 class="mb-2">Ajouter du temps aujourd'hui</h3>
			<div class="grid grid-cols-3 gap-2">
				{#each [15, 30, 60] as m (m)}
					<button
						class="btn-sun display min-h-16 text-xl"
						disabled={busy}
						onclick={() => grant({ kind: 'extra_minutes', minutes: m }, undefined, `${minutes(m)} de plus pour ${name}`)}
					>
						+{minutes(m)}
					</button>
				{/each}
			</div>
		</section>

		{#if status.reason === 'outside_schedule'}
			<section>
				<h3 class="mb-2">Ouvrir en dehors des horaires</h3>
				<div class="grid grid-cols-2 gap-2">
					{#each [30, 60] as m (m)}
						<button
							class="btn-quiet"
							disabled={busy}
							onclick={() => grant({ kind: 'ignore_schedule' }, m, `Ouvert pour ${name} pendant ${minutes(m)}`)}
						>
							Pendant {minutes(m)}
						</button>
					{/each}
				</div>
			</section>
		{/if}

		{#if !pause}
			<section>
				<h3 class="mb-2">Mettre en pause</h3>
				<Segmented
					bind:value={pauseMinutes}
					label="Durée de la pause"
					options={[
						{ value: 30, label: '30 min' },
						{ value: 60, label: '1 h' },
						{ value: 120, label: '2 h' }
					]}
				/>
				<button
					class="btn-quiet btn-block mt-2"
					disabled={busy}
					onclick={() => grant({ kind: 'pause' }, pauseMinutes, `${name} est en pause pendant ${minutes(pauseMinutes)}`)}
				>
					<Pause size={20} /> Mettre en pause {minutes(pauseMinutes)}
				</button>
				<p class="hint">L'écran se verrouille tout de suite, sans toucher à son temps du jour.</p>
			</section>
		{/if}

		{#if others.length > 0}
			<section>
				<h3 class="mb-1">Déjà accordé</h3>
				<div class="rows">
					{#each others as g (g.id)}
						<div class="row justify-between">
							<span>
								<span class="font-semibold">{targetLabel(g.target)}</span>
								<span class="muted block text-sm">{grantRemaining(g, now)}</span>
							</span>
							<button class="btn-ghost btn-sm" disabled={busy} onclick={() => revoke(g, 'Retiré')}>Retirer</button>
						</div>
					{/each}
				</div>
			</section>
		{/if}
	</div>
</Sheet>
