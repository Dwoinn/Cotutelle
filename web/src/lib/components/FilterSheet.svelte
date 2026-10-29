<script lang="ts">
	import type { Grant, Service } from '#lib/types.ts';
	import FilterActions from './FilterActions.svelte';
	import Sheet from './Sheet.svelte';

	// Feuille « Filtre de … » de l'accueil : les actions de filtrage dans une
	// feuille. Fermée, elle oublie l'étape en cours et le site saisi.
	let {
		open = $bindable(false),
		name,
		subject,
		services,
		blockedServices,
		requestableServices = null,
		grants,
		now,
		blockedToday = 0,
		pausable = false,
		activityHref = null,
		ondone
	}: {
		open: boolean;
		name: string;
		subject: { child_id: string } | { device_id: string };
		services: Service[];
		blockedServices: string[];
		requestableServices?: string[] | null;
		grants: Grant[];
		now: number;
		blockedToday?: number;
		pausable?: boolean;
		activityHref?: string | null;
		ondone: () => void;
	} = $props();
</script>

<Sheet bind:open title="Filtre de {name}">
	<FilterActions
		{name}
		{subject}
		{services}
		{blockedServices}
		{requestableServices}
		{grants}
		{now}
		{blockedToday}
		{pausable}
		{activityHref}
		ondone={(close) => {
			if (close) open = false;
			ondone();
		}}
	/>
</Sheet>
