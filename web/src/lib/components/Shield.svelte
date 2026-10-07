<script lang="ts">
	import type { Snippet } from 'svelte';

	// Le bouclier : l'état du filtrage. Deux moitiés, comme dans le logo.
	// Une moitié passe en soleil quand une ouverture temporaire est en cours ;
	// les deux s'estompent quand le filtrage n'est pas actif.
	let {
		size = 132,
		opened = false,
		inactive = false,
		children
	}: { size?: number; opened?: boolean; inactive?: boolean; children?: Snippet } = $props();

	const HALF = 'M26.2 7.2 15.5 10.4Q11 11.2 11 15.8V30.5C11 44.2 19.6 52.6 27.8 57.2';
	const OTHER = 'M37.8 7.2 48.5 10.4Q53 11.2 53 15.8V30.5C53 44.2 44.4 52.6 36.2 57.2';
</script>

<div class="relative shrink-0" style:width="{size}px" style:height="{size}px">
	<svg width={size} height={size} viewBox="3 1.5 58 61" aria-hidden="true" class="block" class:inactive>
		<g fill="none" stroke-width="4.6" stroke-linecap="round" stroke-linejoin="round">
			<path d={HALF} stroke="var(--ink)" />
			<path d={OTHER} stroke={opened ? 'var(--sun)' : 'var(--ink)'} class="half" />
		</g>
	</svg>
	<div class="absolute inset-0 flex flex-col items-center justify-center pb-[6%] text-center">
		{@render children?.()}
	</div>
</div>

<style>
	.half {
		transition: stroke 0.3s ease;
	}
	.inactive {
		opacity: 0.35;
	}
	.inactive path {
		stroke-dasharray: 3 7;
	}
</style>
