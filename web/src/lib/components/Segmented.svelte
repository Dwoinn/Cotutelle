<script lang="ts" generics="T extends string | number">
	// Choix exclusif en grandes cases : durées, onglets.
	let {
		value = $bindable(),
		options,
		label
	}: { value: T; options: { value: T; label: string }[]; label: string } = $props();
</script>

<div class="bg-sunken flex gap-1 rounded-2xl p-1" role="radiogroup" aria-label={label}>
	{#each options as option (option.value)}
		<button
			type="button"
			role="radio"
			aria-checked={value === option.value}
			onclick={() => (value = option.value)}
			class="min-h-11 flex-1 rounded-xl px-2 text-[0.95rem] font-semibold whitespace-nowrap transition-colors {value === option.value
				? 'bg-surface text-ink shadow-sm'
				: 'text-muted'}"
		>
			{option.label}
		</button>
	{/each}
</div>
