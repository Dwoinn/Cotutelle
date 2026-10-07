<script lang="ts">
	import { app } from '#lib/app.svelte.ts';
</script>

<div class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex flex-col items-center gap-2 px-4" aria-live="polite">
	{#each app.toasts as toast (toast.id)}
		<div
			class="pointer-events-auto rounded-full px-4 py-2 text-sm font-medium text-white shadow-lg {toast.kind === 'error'
				? 'bg-rose-600'
				: 'bg-slate-800 dark:bg-slate-700'}"
		>
			{toast.text}
		</div>
	{/each}
</div>
