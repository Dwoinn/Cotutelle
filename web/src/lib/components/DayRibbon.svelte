<script lang="ts">
	import { rangeValid, toMinutes } from '#lib/schedule.ts';
	import type { TimeRange } from '#lib/types.ts';

	// Frise d'une journée : les plages permises sur 24 heures.
	let { ranges }: { ranges: TimeRange[] } = $props();
</script>

<div class="bg-sunken relative h-3 overflow-hidden rounded-full" aria-hidden="true">
	{#each ranges.filter(rangeValid) as r, i (i)}
		<div
			class="bg-ink absolute inset-y-0 rounded-full"
			style:left="{(toMinutes(r.start) / 1440) * 100}%"
			style:width="{((toMinutes(r.end) - toMinutes(r.start)) / 1440) * 100}%"
		></div>
	{/each}
</div>
