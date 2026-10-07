<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		open = $bindable(false),
		title,
		wide = false,
		children
	}: { open: boolean; title: string; wide?: boolean; children: Snippet } = $props();

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') open = false;
	}
</script>

<svelte:window {onkeydown} />

{#if open}
	<div class="fixed inset-0 z-40 flex items-end justify-center bg-slate-950/50 p-0 backdrop-blur-sm sm:items-center sm:p-4">
		<button class="absolute inset-0 cursor-default" aria-label="Fermer" onclick={() => (open = false)}></button>
		<div
			role="dialog"
			aria-modal="true"
			aria-label={title}
			class="relative max-h-[92vh] w-full overflow-y-auto rounded-t-3xl bg-white p-6 shadow-xl sm:rounded-3xl dark:bg-slate-900 {wide
				? 'sm:max-w-2xl'
				: 'sm:max-w-md'}"
		>
			<div class="mb-4 flex items-start justify-between gap-4">
				<h2>{title}</h2>
				<button class="btn-ghost btn-sm -mt-1 -mr-2" onclick={() => (open = false)} aria-label="Fermer">✕</button>
			</div>
			{@render children()}
		</div>
	</div>
{/if}
