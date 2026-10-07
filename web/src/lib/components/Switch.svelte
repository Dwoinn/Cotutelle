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

<button type="button" role="switch" aria-checked={checked} onclick={toggle} class="row justify-between">
	<span class="min-w-0">
		<span class="block font-semibold">{label}</span>
		{#if description}<span class="muted block text-sm">{description}</span>{/if}
	</span>
	<span class="relative h-8 w-14 shrink-0 rounded-full transition-colors {checked ? 'bg-primary' : 'bg-line'}">
		<span
			class="bg-surface absolute top-1 left-1 h-6 w-6 rounded-full shadow-sm transition-transform {checked ? 'translate-x-6' : ''}"
			style:background={checked ? 'var(--on-primary)' : undefined}
		></span>
	</span>
</button>
