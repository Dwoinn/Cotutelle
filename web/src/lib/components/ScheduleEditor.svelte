<script lang="ts">
	import { DAY_LABELS, hm, minutes } from '#lib/format.ts';
	import { ALL_DAY, FREE_DAYS, SCHOOL_DAYS, isAllDay, rangeValid, toMinutes, totalMinutes } from '#lib/schedule.ts';
	import { DAYS, type Day, type Schedule, type TimeRange } from '#lib/types.ts';

	// Éditeur des plages autorisées, jour par jour.
	let { schedule = $bindable() }: { schedule: Schedule } = $props();

	function addRange(day: Day) {
		const last = schedule[day].at(-1);
		const start = last ? Math.min(toMinutes(last.end) + 60, 22 * 60) : 17 * 60;
		const end = Math.min(start + 120, 24 * 60);
		const fmt = (m: number) => `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}:00`;
		schedule[day] = [...schedule[day], { start: fmt(start), end: end >= 24 * 60 ? null : fmt(end) }];
	}

	function removeRange(day: Day, index: number) {
		schedule[day] = schedule[day].filter((_, i) => i !== index);
	}

	function setStart(range: TimeRange, value: string) {
		if (value) range.start = `${value}:00`;
	}

	// Une fin à 00:00 signifie « jusqu'à minuit ».
	function setEnd(range: TimeRange, value: string) {
		if (value) range.end = value === '00:00' ? null : `${value}:00`;
	}

	function copy(from: Day, targets: Day[]) {
		for (const day of targets) {
			if (day !== from) schedule[day] = schedule[from].map((r) => ({ ...r }));
		}
	}

	function onCopy(from: Day, event: Event) {
		const select = event.currentTarget as HTMLSelectElement;
		const targets: Record<string, Day[]> = { all: [...DAYS], school: SCHOOL_DAYS, free: FREE_DAYS };
		if (targets[select.value]) copy(from, targets[select.value]);
		select.value = '';
	}
</script>

<div class="space-y-3">
	{#each DAYS as day (day)}
		{@const ranges = schedule[day]}
		<div class="rounded-2xl border border-slate-200 p-3 dark:border-slate-800">
			<div class="flex flex-wrap items-center justify-between gap-2">
				<div class="flex items-baseline gap-2">
					<span class="w-20 text-sm font-semibold">{DAY_LABELS[day]}</span>
					<span class="muted text-xs">
						{#if ranges.length === 0}
							aucun accès
						{:else if isAllDay(ranges)}
							toute la journée
						{:else}
							{minutes(totalMinutes(ranges))} possibles
						{/if}
					</span>
				</div>
				<div class="flex items-center gap-1">
					<button class="btn-ghost btn-sm" onclick={() => addRange(day)}>+ Plage</button>
					{#if !isAllDay(ranges)}
						<button class="btn-ghost btn-sm" onclick={() => (schedule[day] = [{ ...ALL_DAY }])}>Toute la journée</button>
					{/if}
					<select
						class="btn-ghost btn-sm appearance-none bg-transparent"
						aria-label="Copier {DAY_LABELS[day]} vers d'autres jours"
						onchange={(e) => onCopy(day, e)}
					>
						<option value="">Copier vers…</option>
						<option value="all">Tous les jours</option>
						<option value="school">Jours d'école</option>
						<option value="free">Mercredi et week-end</option>
					</select>
				</div>
			</div>

			<!-- Frise de la journée : les plages autorisées en couleur. -->
			<div class="relative mt-2 h-2.5 overflow-hidden rounded-full bg-slate-100 dark:bg-slate-800" aria-hidden="true">
				{#each ranges.filter(rangeValid) as r, i (i)}
					<div
						class="bg-brand-400 absolute inset-y-0 rounded-full"
						style:left="{(toMinutes(r.start) / 1440) * 100}%"
						style:width="{((toMinutes(r.end) - toMinutes(r.start)) / 1440) * 100}%"
					></div>
				{/each}
			</div>
			<div class="muted mt-0.5 flex justify-between text-[10px]" aria-hidden="true">
				<span>0 h</span><span>6 h</span><span>12 h</span><span>18 h</span><span>24 h</span>
			</div>

			{#if ranges.length > 0 && !isAllDay(ranges)}
				<div class="mt-2 flex flex-wrap gap-2">
					{#each ranges as r, i (i)}
						<div
							class="flex items-center gap-1 rounded-full border px-2 py-1 text-sm {rangeValid(r)
								? 'border-slate-200 dark:border-slate-700'
								: 'border-rose-400 bg-rose-50 dark:bg-rose-950/40'}"
						>
							<input
								type="time"
								class="bg-transparent outline-none"
								aria-label="Début"
								value={hm(r.start)}
								onchange={(e) => setStart(r, e.currentTarget.value)}
							/>
							<span class="muted">→</span>
							<input
								type="time"
								class="bg-transparent outline-none"
								aria-label="Fin"
								value={r.end === null ? '00:00' : hm(r.end)}
								onchange={(e) => setEnd(r, e.currentTarget.value)}
							/>
							<button class="muted px-1 hover:text-rose-600" aria-label="Supprimer la plage" onclick={() => removeRange(day, i)}>
								✕
							</button>
						</div>
					{/each}
				</div>
			{:else if isAllDay(ranges)}
				<button class="muted mt-2 text-xs underline" onclick={() => (schedule[day] = [])}>Bloquer toute la journée</button>
			{/if}
		</div>
	{/each}
</div>
