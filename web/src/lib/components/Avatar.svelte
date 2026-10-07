<script lang="ts">
	import { initial } from '#lib/format.ts';

	// Monogramme d'un enfant sur sa couleur.
	let { name, color, size = 44 }: { name: string; color: string; size?: number } = $props();
</script>

<span
	class="display inline-flex shrink-0 items-center justify-center text-white"
	style:width="{size}px"
	style:height="{size}px"
	style:border-radius="{size * 0.36}px"
	style:font-size="{size * 0.5}px"
	style:background={color}
	aria-hidden="true"
>
	{initial(name)}
</span>
