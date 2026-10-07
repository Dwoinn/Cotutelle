<script lang="ts">
	import { seconds, shortDay } from '#lib/format.ts';

	// Temps d'écran par jour, en barres. `quota` (minutes) trace un repère.
	let {
		days,
		quota = null,
		color = 'var(--color-brand-500)'
	}: { days: { day: string; seconds: number }[]; quota?: number | null; color?: string } = $props();

	const max = $derived(Math.max(...days.map((d) => d.seconds), (quota ?? 0) * 60, 60 * 30));
	const total = $derived(days.reduce((sum, d) => sum + d.seconds, 0));
	// Au-delà de deux semaines, une étiquette sur trois suffit.
	const step = $derived(days.length > 14 ? 5 : 1);
</script>

<div>
	<div class="relative flex h-36 items-end gap-1.5">
		{#if quota}
			<div
				class="pointer-events-none absolute inset-x-0 border-t border-dashed border-slate-300 dark:border-slate-600"
				style:bottom="{((quota * 60) / max) * 100}%"
			>
				<span class="muted absolute -top-4 right-0 text-[10px]">quota {quota} min</span>
			</div>
		{/if}
		{#each days as d (d.day)}
			<div class="group relative flex h-full flex-1 flex-col justify-end">
				<div
					class="min-h-0.5 rounded-t-lg transition-all"
					style:height="{(d.seconds / max) * 100}%"
					style:background={d.seconds > 0 ? color : 'transparent'}
					style:opacity={quota && d.seconds > quota * 60 ? 1 : 0.75}
				></div>
				<div
					class="pointer-events-none absolute -top-1 left-1/2 z-10 hidden -translate-x-1/2 -translate-y-full rounded-lg bg-slate-800 px-2 py-1 text-xs whitespace-nowrap text-white group-hover:block"
				>
					{shortDay(d.day)} · {seconds(d.seconds)}
				</div>
			</div>
		{/each}
	</div>
	<div class="mt-1 flex gap-1.5">
		{#each days as d, i (d.day)}
			<div class="muted flex-1 truncate text-center text-[10px]">{i % step === 0 ? shortDay(d.day) : ''}</div>
		{/each}
	</div>
	<p class="muted mt-2">
		Total : <strong class="text-slate-700 dark:text-slate-200">{seconds(total)}</strong>
		· moyenne {seconds(total / Math.max(1, days.length))} par jour
	</p>
</div>
