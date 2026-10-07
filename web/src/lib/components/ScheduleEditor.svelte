<script lang="ts">
	import { DAY_LABELS, hm, minutes, range } from '#lib/format.ts';
	import { ALL_DAY, FREE_DAYS, SCHOOL_DAYS, isAllDay, rangeValid, toMinutes, totalMinutes } from '#lib/schedule.ts';
	import { DAYS, type Day, type Schedule, type TimeRange } from '#lib/types.ts';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash from '@lucide/svelte/icons/trash';
	import DayRibbon from './DayRibbon.svelte';
	import Sheet from './Sheet.svelte';

	// Horaires de la semaine : une ligne par jour, modifiée dans une feuille.
	let { schedule = $bindable() }: { schedule: Schedule } = $props();

	let editing = $state<Day | null>(null);
	let sheetOpen = $state(false);
	let copied = $state<string | null>(null);

	function edit(day: Day) {
		editing = day;
		copied = null;
		sheetOpen = true;
	}

	function summary(ranges: TimeRange[]): string {
		if (ranges.length === 0) return 'Aucun accès';
		if (isAllDay(ranges)) return 'Toute la journée';
		return ranges.map(range).join(', ');
	}

	const fmt = (m: number) => `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}:00`;

	function addRange(day: Day) {
		const last = schedule[day].at(-1);
		const start = last ? Math.min(toMinutes(last.end) + 60, 22 * 60) : 17 * 60;
		const end = Math.min(start + 120, 24 * 60);
		const base = isAllDay(schedule[day]) ? [] : schedule[day];
		schedule[day] = [...base, { start: fmt(start), end: end >= 24 * 60 ? null : fmt(end) }];
	}

	function setStart(r: TimeRange, value: string) {
		if (value) r.start = `${value}:00`;
	}

	// Une fin à 00:00 signifie « jusqu'à minuit ».
	function setEnd(r: TimeRange, value: string) {
		if (value) r.end = value === '00:00' ? null : `${value}:00`;
	}

	function copyTo(from: Day, targets: Day[], label: string) {
		for (const day of targets) {
			if (day !== from) schedule[day] = schedule[from].map((r) => ({ ...r }));
		}
		copied = label;
	}
</script>

<div class="rows">
	{#each DAYS as day (day)}
		{@const ranges = schedule[day]}
		<button class="row" onclick={() => edit(day)}>
			<span class="min-w-0 flex-1">
				<span class="flex items-baseline justify-between gap-3">
					<span class="font-semibold">{DAY_LABELS[day]}</span>
					<span class="text-sm {ranges.every(rangeValid) ? 'muted' : 'text-cherry font-semibold'}">
						{ranges.every(rangeValid) ? summary(ranges) : 'Plage invalide'}
					</span>
				</span>
				<span class="mt-2 block"><DayRibbon {ranges} /></span>
			</span>
			<ChevronRight size={20} class="text-muted shrink-0" />
		</button>
	{/each}
</div>
<div class="muted mt-1 flex justify-between pr-8 text-xs" aria-hidden="true">
	<span>0 h</span><span>6 h</span><span>12 h</span><span>18 h</span><span>24 h</span>
</div>

<Sheet bind:open={sheetOpen} title={editing ? DAY_LABELS[editing] : ''}>
	{#if editing}
		{@const day = editing}
		{@const ranges = schedule[day]}
		<div class="space-y-5">
			<div>
				<DayRibbon {ranges} />
				<p class="muted mt-2 text-sm">
					{#if ranges.length === 0}
						Aucun accès ce jour-là.
					{:else if isAllDay(ranges)}
						Accès toute la journée, dans la limite du temps d'écran.
					{:else}
						{minutes(totalMinutes(ranges))} de plage en tout.
					{/if}
				</p>
			</div>

			{#if ranges.length > 0 && !isAllDay(ranges)}
				<div class="space-y-2">
					{#each ranges as r, i (i)}
						<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-end gap-2">
							<label class="min-w-0">
								<span class="label">De</span>
								<input type="time" class="field px-2 {rangeValid(r) ? '' : 'border-cherry'}" value={hm(r.start)} onchange={(e) => setStart(r, e.currentTarget.value)} />
							</label>
							<label class="min-w-0">
								<span class="label">À</span>
								<input type="time" class="field px-2 {rangeValid(r) ? '' : 'border-cherry'}" value={r.end === null ? '00:00' : hm(r.end)} onchange={(e) => setEnd(r, e.currentTarget.value)} />
							</label>
							<button class="icon-btn mb-0.5" aria-label="Supprimer cette plage" onclick={() => (schedule[day] = schedule[day].filter((_, j) => j !== i))}>
								<Trash size={20} />
							</button>
						</div>
						{#if !rangeValid(r)}
							<p class="text-cherry text-sm">La fin doit être après le début.</p>
						{/if}
					{/each}
				</div>
			{/if}

			<div class="grid gap-2">
				<button class="btn-quiet" onclick={() => addRange(day)}><Plus size={20} /> Ajouter une plage</button>
				<div class="grid grid-cols-2 gap-2">
					<button class="btn-ghost border-line border" disabled={isAllDay(ranges)} onclick={() => (schedule[day] = [{ ...ALL_DAY }])}>Toute la journée</button>
					<button class="btn-ghost border-line border" disabled={ranges.length === 0} onclick={() => (schedule[day] = [])}>Aucun accès</button>
				</div>
			</div>

			<section>
				<h3 class="mb-2">Appliquer aussi à</h3>
				<div class="grid gap-2">
					<button class="btn-quiet" onclick={() => copyTo(day, SCHOOL_DAYS, "les jours d'école")}>Lundi, mardi, jeudi, vendredi</button>
					<button class="btn-quiet" onclick={() => copyTo(day, FREE_DAYS, 'mercredi et le week-end')}>Mercredi, samedi, dimanche</button>
					<button class="btn-quiet" onclick={() => copyTo(day, [...DAYS], 'toute la semaine')}>Toute la semaine</button>
				</div>
				{#if copied}<p class="text-mint mt-2 text-sm font-semibold" role="status">Copié sur {copied}.</p>{/if}
			</section>

			<button class="btn-primary btn-block" onclick={() => (sheetOpen = false)}>Terminé</button>
		</div>
	{/if}
</Sheet>
