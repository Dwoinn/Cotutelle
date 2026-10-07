<script lang="ts">
	// Mentions légales affichées par l'interface (articles 5(d), 7(b) et 13 de
	// l'AGPL). Une version modifiée doit conserver l'attribution à Cotutelle,
	// sous la forme « Basé sur Cotutelle » avec ce lien, et faire pointer
	// « Code source » vers son propre dépôt. Voir NOTICE.md.
	const PROJECT = 'https://github.com/Dwoinn/cotutelle';
	const SOURCE = PROJECT;

	let { version = null }: { version?: string | null } = $props();
</script>

<footer class="muted mt-8 text-center text-xs leading-relaxed">
	<a class="font-medium hover:underline" href={PROJECT} target="_blank" rel="noopener">Cotutelle</a>{#if version}&nbsp;{version}{/if}
	· logiciel libre sous licence
	<a class="hover:underline" href="https://www.gnu.org/licenses/agpl-3.0.html" target="_blank" rel="noopener">AGPL-3.0</a>,
	sans aucune garantie ·
	<a class="hover:underline" href={SOURCE} target="_blank" rel="noopener">Code source</a>
</footer>
