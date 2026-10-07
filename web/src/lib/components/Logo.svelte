<script lang="ts">
	// La marque : deux moitiés de bouclier, comme deux bras, autour d'un cadran.
	// Le bouclier prend la couleur du texte ; le cadran reste soleil.
	let { size = 32, wordmark = false }: { size?: number; wordmark?: boolean } = $props();
</script>

<span class="inline-flex items-center" style:gap="{size * 0.28}px">
	<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true" class="shrink-0">
		<g fill="none" stroke="currentColor" stroke-width="6.6" stroke-linecap="round" stroke-linejoin="round">
			<path d="M26.2 7.2 15.5 10.4Q11 11.2 11 15.8V30.5C11 44.2 19.6 52.6 27.8 57.2" />
			<path d="M37.8 7.2 48.5 10.4Q53 11.2 53 15.8V30.5C53 44.2 44.4 52.6 36.2 57.2" />
		</g>
		<circle cx="32" cy="30.8" r="10.4" fill="#ffc53d" />
		<path d="M32 24.6V30.8L36.4 33.4" fill="none" stroke="#16203a" stroke-width="3.3" stroke-linecap="round" stroke-linejoin="round" />
	</svg>
	{#if wordmark}
		<span class="display" style:font-size="{size * 0.78}px">Cotutelle</span>
	{:else}
		<span class="sr-only">Cotutelle</span>
	{/if}
</span>
