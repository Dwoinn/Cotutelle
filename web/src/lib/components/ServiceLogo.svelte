<script lang="ts">
	import { initial } from '#lib/format.ts';
	import type { Logo } from '#lib/types.ts';

	// Pastille d'un service : son logo sur fond clair, ou son initiale tant
	// que le serveur n'en a pas trouvé.
	let { label, logo = null, size = 40 }: { label: string; logo?: Logo | null; size?: number } = $props();

	let broken = $state<string | null>(null);
	const shown = $derived(logo && logo.url !== broken ? logo : null);
</script>

<span
	class="pastille"
	class:display={!shown}
	class:initiale={!shown}
	style:width="{size}px"
	style:height="{size}px"
	style:border-radius="{size * 0.28}px"
	style:font-size="{size * 0.5}px"
	aria-hidden="true"
>
	{#if shown}
		<img src={shown.url} alt="" class:bord={shown.bleed} loading="lazy" decoding="async" onerror={() => (broken = shown.url)} />
	{:else}
		{initial(label)}
	{/if}
</span>

<style>
	/* Les logos sont dessinés pour un fond clair : la pastille le reste, même
	   dans le thème sombre. */
	.pastille {
		position: relative;
		display: inline-flex;
		flex-shrink: 0;
		align-items: center;
		justify-content: center;
		overflow: hidden;
		background: #fff;
	}
	.pastille::after {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: inherit;
		box-shadow: inset 0 0 0 1px rgb(22 32 58 / 0.1);
	}
	.initiale {
		background: var(--ink);
		color: var(--surface);
	}
	.initiale::after {
		content: none;
	}
	img {
		width: 68%;
		height: 68%;
		object-fit: contain;
	}
	img.bord {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
</style>
