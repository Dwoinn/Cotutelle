<script lang="ts">
	import { app } from '#lib/app.svelte.ts';
</script>

<div
	class="pointer-events-none fixed inset-x-0 bottom-[calc(5.5rem+env(safe-area-inset-bottom))] z-50 flex flex-col items-center gap-2 px-4 sm:bottom-6"
	aria-live="polite"
>
	{#each app.toasts as toast (toast.id)}
		<div
			class="sheet pointer-events-auto max-w-sm rounded-2xl px-4 py-3 text-center text-[0.95rem] font-semibold shadow-lg"
			style:background={toast.kind === 'error' ? 'var(--cherry)' : '#16203a'}
			style:color="#fff"
		>
			{toast.text}
		</div>
	{/each}
</div>
