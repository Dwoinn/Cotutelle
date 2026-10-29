<script lang="ts">
	import type { AccessStatus, Grant } from '#lib/types.ts';
	import Sheet from './Sheet.svelte';
	import TimeActions from './TimeActions.svelte';

	// Feuille « Temps de … » de l'accueil : les actions de temps dans une feuille.
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
		line: string;
		grants: Grant[];
		now: number;
		ondone: () => void;
	} = $props();
</script>

<Sheet bind:open title="Temps de {name}">
	<TimeActions
		{name}
		{childId}
		{status}
		{line}
		{grants}
		{now}
		ondone={(close) => {
			if (close) open = false;
			ondone();
		}}
	/>
</Sheet>
