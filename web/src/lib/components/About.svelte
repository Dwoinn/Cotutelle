<script lang="ts">
	// Mentions légales affichées par l'interface (articles 5(d), 7(b) et 13 de
	// l'AGPL). Une version modifiée doit conserver l'attribution à Cotutelle,
	// sous la forme « Basé sur Cotutelle » avec ce lien, et faire pointer
	// « Code source » vers son propre dépôt. Voir NOTICE.md.
	const PROJECT = 'https://github.com/Dwoinn/cotutelle';
	const SOURCE = PROJECT;

	let { version = null }: { version?: string | null } = $props();
</script>

<footer class="muted mt-10 text-center text-sm leading-relaxed">
	<p>
		<a class="font-semibold underline-offset-2 hover:underline" href={PROJECT} target="_blank" rel="noopener">Cotutelle</a>{#if version}&nbsp;{version}{/if},
		logiciel libre sous licence
		<a class="underline-offset-2 hover:underline" href="https://www.gnu.org/licenses/agpl-3.0.html" target="_blank" rel="noopener">AGPL-3.0</a>, sans aucune garantie.
	</p>
	<p><a class="underline underline-offset-2" href={SOURCE} target="_blank" rel="noopener">Code source</a></p>
</footer>
