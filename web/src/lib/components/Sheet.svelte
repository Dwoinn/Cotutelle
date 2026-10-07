<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import type { Snippet } from 'svelte';

	// Feuille : monte du bas de l'écran sur téléphone, se centre sur grand écran.
	let {
		open = $bindable(false),
		title,
		wide = false,
		children
	}: { open: boolean; title: string; wide?: boolean; children: Snippet } = $props();

	let panel = $state<HTMLDivElement | null>(null);

	function onkeydown(event: KeyboardEvent) {
		if (open && event.key === 'Escape') open = false;
	}

	// Fige la page derrière la feuille et y place le focus.
	$effect(() => {
		if (!open) return;
		const previous = document.body.style.overflow;
		document.body.style.overflow = 'hidden';
		panel?.focus();
		return () => {
			document.body.style.overflow = previous;
		};
	});
</script>

<svelte:window {onkeydown} />

{#if open}
	<div class="fixed inset-0 z-40 flex items-end justify-center sm:items-center sm:p-6">
		<button class="scrim absolute inset-0 cursor-default" style:background="var(--scrim)" aria-label="Fermer" onclick={() => (open = false)}></button>
		<div
			bind:this={panel}
			tabindex="-1"
			role="dialog"
			aria-modal="true"
			aria-label={title}
			class="sheet bg-surface relative max-h-[92dvh] w-full overflow-y-auto rounded-t-[28px] px-5 pt-3 pb-[max(1.25rem,env(safe-area-inset-bottom))] outline-none sm:rounded-[28px] sm:p-6 {wide
				? 'sm:max-w-2xl'
				: 'sm:max-w-md'}"
		>
			<div class="bg-line mx-auto mb-2 h-1.5 w-10 rounded-full sm:hidden" aria-hidden="true"></div>
			<div class="mb-4 flex items-center justify-between gap-3">
				<h2>{title}</h2>
				<button class="icon-btn -mr-2" onclick={() => (open = false)} aria-label="Fermer"><X size={22} /></button>
			</div>
			{@render children()}
		</div>
	</div>
{/if}
