<script lang="ts">
	import type { ServiceMode } from '#lib/services.ts';
	import Shield from './Shield.svelte';

	// Interrupteur à trois positions : ce que le filtre fait d'un service.
	// Chaque position reprend un état du bouclier : en pointillés quand il
	// laisse passer, à moitié soleil quand il s'ouvre sur demande, plein
	// quand il bloque.
	let {
		value,
		name,
		ask = true,
		onchange
	}: {
		value: ServiceMode;
		/** Nom du service, pour les lecteurs d'écran. */
		name: string;
		/** Faux pour un appareil partagé : personne n'y fait de demande. */
		ask?: boolean;
		onchange: (mode: ServiceMode) => void;
	} = $props();

	const POSITIONS: { value: ServiceMode; label: string }[] = [
		{ value: 'open', label: 'Autorisé' },
		{ value: 'ask', label: 'Sur demande' },
		{ value: 'blocked', label: 'Bloqué' }
	];
	const positions = $derived(POSITIONS.filter((p) => ask || p.value !== 'ask'));
	const current = $derived(!ask && value === 'ask' ? 'blocked' : value);
</script>

<div class="bg-sunken flex shrink-0 gap-0.5 rounded-[15px] p-[3px]" role="radiogroup" aria-label={name}>
	{#each positions as position (position.value)}
		{@const on = current === position.value}
		<button
			type="button"
			role="radio"
			aria-checked={on}
			aria-label={position.label}
			title={position.label}
			class="flex h-11 w-11 items-center justify-center rounded-xl transition-colors {on ? 'bg-surface ring-line shadow-sm ring-1' : ''}"
			onclick={() => onchange(position.value)}
		>
			{#if on}
				<Shield size={25} opened={position.value === 'ask'} inactive={position.value === 'open'} />
			{:else}
				<span class="bg-muted h-1.5 w-1.5 rounded-full opacity-40"></span>
			{/if}
		</button>
	{/each}
</div>
