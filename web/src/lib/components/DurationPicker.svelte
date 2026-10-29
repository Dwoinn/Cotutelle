<script lang="ts">
	import { minutes } from '#lib/format.ts';

	// Choix d'une durée en un geste : un bouton par durée, qui agit tout de suite.
	let {
		options = [15, 30, 60, 120],
		busy = false,
		onpick
	}: { options?: number[]; busy?: boolean; onpick: (minutes: number) => void } = $props();
</script>

<div class="grid gap-2" style:grid-template-columns="repeat({options.length}, minmax(0, 1fr))">
	{#each options as m (m)}
		<button class="btn-sun display min-h-16 px-1 text-xl" disabled={busy} onclick={() => onpick(m)}>
			{minutes(m)}
		</button>
	{/each}
</div>
