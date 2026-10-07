<script lang="ts">
	import { minutes, seconds, shortDay } from '#lib/format.ts';

	// Temps d'écran par jour. Toucher une barre affiche sa valeur : rien ne
	// dépend du survol, pour rester utilisable au doigt.
	let { days, quota = null }: { days: { day: string; seconds: number }[]; quota?: number | null } = $props();

	let selected = $state<string | null>(null);

	const max = $derived(Math.max(...days.map((d) => d.seconds), (quota ?? 0) * 60, 30 * 60));
	const total = $derived(days.reduce((sum, d) => sum + d.seconds, 0));
	const current = $derived(days.find((d) => d.day === selected) ?? null);
	const dense = $derived(days.length > 10);
</script>

<div>
	<p class="mb-3 min-h-[2.6rem]">
		{#if current}
			<span class="display text-3xl">{seconds(current.seconds)}</span>
			<span class="muted ml-1 text-sm">{shortDay(current.day)}</span>
		{:else}
			<span class="display text-3xl">{seconds(total / Math.max(1, days.length))}</span>
			<span class="muted ml-1 text-sm">par jour en moyenne</span>
		{/if}
	</p>
	<div class="relative flex h-36 items-end" class:gap-1={dense} class:gap-2={!dense}>
		{#if quota}
			<div class="border-muted pointer-events-none absolute inset-x-0 border-t border-dashed opacity-50" style:bottom="{((quota * 60) / max) * 100}%"></div>
		{/if}
		{#each days as d (d.day)}
			<button
				type="button"
				class="flex h-full flex-1 flex-col justify-end rounded-lg"
				aria-label="{shortDay(d.day)} : {seconds(d.seconds)}"
				aria-pressed={selected === d.day}
				onclick={() => (selected = selected === d.day ? null : d.day)}
			>
				<span
					class="block min-h-1 w-full rounded-t-md rounded-b-sm transition-all"
					style:height="{Math.max(2, (d.seconds / max) * 100)}%"
					style:background={selected === d.day ? 'var(--ink)' : quota && d.seconds > quota * 60 ? 'var(--ember)' : 'var(--sun)'}
					style:opacity={d.seconds === 0 ? 0.25 : 1}
				></span>
			</button>
		{/each}
	</div>
	{#if !dense}
		<div class="mt-1.5 flex gap-2">
			{#each days as d (d.day)}
				<span class="muted flex-1 truncate text-center text-xs">{shortDay(d.day).split(' ')[0]}</span>
			{/each}
		</div>
	{/if}
	{#if quota}
		<p class="muted mt-2 text-sm">Le trait marque la limite de {minutes(quota)} par jour.</p>
	{/if}
</div>
