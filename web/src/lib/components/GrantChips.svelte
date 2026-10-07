<script lang="ts">
	import { clock, targetIcon, targetLabel, until } from '#lib/format.ts';
	import type { Grant, Service } from '#lib/types.ts';

	// Exceptions en cours, avec leur échéance et un bouton pour les retirer.
	let {
		grants,
		services = [],
		now,
		onrevoke
	}: { grants: Grant[]; services?: Service[]; now: number; onrevoke?: (grant: Grant) => void } = $props();
</script>

{#if grants.length > 0}
	<div class="flex flex-wrap gap-2">
		{#each grants as grant (grant.id)}
			<span
				class="chip {grant.target.kind === 'pause'
					? 'bg-amber-50 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300'
					: 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300'}"
				title="Jusqu'à {clock(grant.expires_at)}{grant.granted_by ? ` · accordé par ${grant.granted_by}` : ''}"
			>
				<span>{targetIcon(grant.target, services)}</span>
				<span>{targetLabel(grant.target, services)}</span>
				<span class="opacity-70">
					{grant.target.kind === 'extra_minutes' ? "aujourd'hui" : `encore ${until(grant.expires_at, now)}`}
				</span>
				{#if onrevoke}
					<button class="-mr-1 rounded-full px-1 hover:bg-black/10" aria-label="Retirer" onclick={() => onrevoke(grant)}>
						✕
					</button>
				{/if}
			</span>
		{/each}
	</div>
{/if}
