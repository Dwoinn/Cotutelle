<script lang="ts">
	import type { Snippet } from 'svelte';

	// Anneau de progression : `value` entre 0 et 1.
	let {
		value,
		size = 120,
		stroke = 10,
		color = 'var(--color-brand-500)',
		children
	}: { value: number; size?: number; stroke?: number; color?: string; children?: Snippet } = $props();

	const radius = $derived((size - stroke) / 2);
	const circumference = $derived(2 * Math.PI * radius);
	const clamped = $derived(Math.min(1, Math.max(0, value)));
</script>

<div class="relative shrink-0" style:width="{size}px" style:height="{size}px">
	<svg width={size} height={size} class="-rotate-90" aria-hidden="true">
		<circle
			cx={size / 2}
			cy={size / 2}
			r={radius}
			fill="none"
			stroke-width={stroke}
			class="stroke-slate-100 dark:stroke-slate-800"
		/>
		<circle
			cx={size / 2}
			cy={size / 2}
			r={radius}
			fill="none"
			stroke={color}
			stroke-width={stroke}
			stroke-linecap="round"
			stroke-dasharray={circumference}
			stroke-dashoffset={circumference * (1 - clamped)}
			style="transition: stroke-dashoffset 0.6s ease"
		/>
	</svg>
	<div class="absolute inset-0 flex flex-col items-center justify-center text-center">
		{@render children?.()}
	</div>
</div>
