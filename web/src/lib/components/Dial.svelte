<script lang="ts">
	import { rangeValid, toMinutes } from '#lib/schedule.ts';
	import type { TimeRange } from '#lib/types.ts';
	import type { Snippet } from 'svelte';
	import { onMount } from 'svelte';

	// Le cadran : une journée de 24 heures, midi en haut, minuit en bas.
	// Anneau extérieur : les plages permises et un repère à l'heure qu'il est.
	// Anneau intérieur : le temps restant. Au centre, le contenu passé par l'appelant.
	let {
		size = 132,
		ranges = [],
		now = null,
		remaining = null,
		open = true,
		low = false,
		labels = false,
		children
	}: {
		size?: number;
		ranges?: TimeRange[];
		/** Minutes depuis minuit ; `null` masque le repère. */
		now?: number | null;
		/** Part du quota restante, de 0 à 1 ; `null` = pas de quota. */
		remaining?: number | null;
		open?: boolean;
		/** Bientôt fini : l'anneau passe en braise. */
		low?: boolean;
		/** Affiche 0, 6, 12, 18 autour du cadran. */
		labels?: boolean;
		children?: Snippet;
	} = $props();

	const C = 100;
	const R_DAY = 91;
	const R_TIME = 70;
	const TIME_LENGTH = 2 * Math.PI * R_TIME;

	// Angle en radians pour une minute de la journée, midi en haut, sens horaire.
	const theta = (minute: number) => ((minute - 720) / 1440) * 2 * Math.PI - Math.PI / 2;
	const at = (radius: number, minute: number) => [C + radius * Math.cos(theta(minute)), C + radius * Math.sin(theta(minute))];

	function arc(from: number, to: number): string {
		const [x1, y1] = at(R_DAY, from);
		const [x2, y2] = at(R_DAY, to);
		return `M${x1.toFixed(2)} ${y1.toFixed(2)}A${R_DAY} ${R_DAY} 0 ${to - from > 720 ? 1 : 0} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
	}

	const windows = $derived(
		ranges.filter(rangeValid).map((r) => ({ from: toMinutes(r.start), to: toMinutes(r.end) }))
	);
	const wholeDay = $derived(windows.some((w) => w.to - w.from >= 1439));
	const ticks = Array.from({ length: 24 }, (_, h) => {
		const major = h % 6 === 0;
		const [x1, y1] = at(major ? 79.5 : 82, h * 60);
		const [x2, y2] = at(84.5, h * 60);
		return { x1, y1, x2, y2, major };
	});
	const marker = $derived(now === null ? null : at(R_DAY, now));
	const fraction = $derived(!open ? 0 : remaining === null ? 1 : Math.min(1, Math.max(0, remaining)));

	// Le seul moment animé de l'interface : le cadran se dessine à l'arrivée.
	let drawn = $state(false);
	onMount(() => {
		const frame = requestAnimationFrame(() => (drawn = true));
		return () => cancelAnimationFrame(frame);
	});

	const box = $derived(labels ? '-18 -18 236 236' : '0 0 200 200');
	const LABELS = [
		{ text: '12', minute: 720 },
		{ text: '18', minute: 1080 },
		{ text: '0', minute: 0 },
		{ text: '6', minute: 360 }
	];
</script>

<div class="relative shrink-0" style:width="{size}px" style:height="{size}px">
	<svg width={size} height={size} viewBox={box} aria-hidden="true" class="block">
		<!-- La journée -->
		<circle cx={C} cy={C} r={R_DAY} fill="none" stroke="var(--line)" stroke-width="2" />
		{#each ticks as t, i (i)}
			<line x1={t.x1} y1={t.y1} x2={t.x2} y2={t.y2} stroke="var(--muted)" stroke-width={t.major ? 2 : 1.2} stroke-linecap="round" opacity={t.major ? 0.7 : 0.35} />
		{/each}
		<g>
			{#if wholeDay}
				<circle cx={C} cy={C} r={R_DAY} fill="none" stroke="var(--ink)" stroke-width="5" />
			{:else}
				{#each windows as w, i (i)}
					<path d={arc(w.from, w.to)} fill="none" stroke="var(--ink)" stroke-width="5" stroke-linecap="round" />
				{/each}
			{/if}
		</g>

		<!-- Le temps restant -->
		<circle cx={C} cy={C} r={R_TIME} fill="none" stroke="var(--sunken)" stroke-width="15" />
		<circle
			class="time"
			cx={C}
			cy={C}
			r={R_TIME}
			fill="none"
			stroke={low ? 'var(--ember)' : 'var(--sun)'}
			stroke-width="15"
			stroke-linecap="round"
			stroke-dasharray="{TIME_LENGTH} {TIME_LENGTH}"
			stroke-dashoffset={TIME_LENGTH * (1 - (drawn ? fraction : 0))}
			transform="rotate(-90 {C} {C})"
			opacity={fraction === 0 ? 0 : 1}
		/>

		{#if marker}
			<circle cx={marker[0]} cy={marker[1]} r="8" fill="var(--sun)" stroke="var(--surface)" stroke-width="3.5" />
		{/if}

		{#if labels}
			{#each LABELS as l (l.text)}
				{@const [x, y] = at(108, l.minute)}
				<text {x} y={y + 4.5} text-anchor="middle" font-size="13" font-weight="600" fill="var(--muted)" class="font-sans">{l.text}</text>
			{/each}
		{/if}
	</svg>
	<div class="absolute inset-0 flex flex-col items-center justify-center text-center">
		{@render children?.()}
	</div>
</div>

<style>
	.time {
		transition:
			stroke-dashoffset 0.9s cubic-bezier(0.2, 0.8, 0.2, 1),
			stroke 0.3s ease;
	}
</style>
