<script lang="ts">
	let {
		checked = $bindable(false),
		label,
		description,
		onchange
	}: { checked: boolean; label: string; description?: string; onchange?: (value: boolean) => void } = $props();

	function toggle() {
		checked = !checked;
		onchange?.(checked);
	}
</script>

<button
	type="button"
	role="switch"
	aria-checked={checked}
	onclick={toggle}
	class="flex w-full items-center justify-between gap-4 rounded-2xl px-1 py-2 text-left"
>
	<span>
		<span class="block text-sm font-medium">{label}</span>
		{#if description}<span class="muted block text-xs">{description}</span>{/if}
	</span>
	<span
		class="relative h-6 w-11 shrink-0 rounded-full transition {checked
			? 'bg-brand-500'
			: 'bg-slate-300 dark:bg-slate-700'}"
	>
		<span
			class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform {checked
				? 'translate-x-5'
				: ''}"
		></span>
	</span>
</button>
