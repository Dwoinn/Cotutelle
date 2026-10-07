<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt, catalog as loadCatalog } from '#lib/app.svelte.ts';
	import FilterEditor from '#lib/components/FilterEditor.svelte';
	import ScheduleEditor from '#lib/components/ScheduleEditor.svelte';
	import Sheet from '#lib/components/Sheet.svelte';
	import { ago, clock } from '#lib/format.ts';
	import { scheduleValid } from '#lib/schedule.ts';
	import type { Catalog, Child, Device, Policy } from '#lib/types.ts';
	import Laptop from '@lucide/svelte/icons/laptop';
	import Plus from '@lucide/svelte/icons/plus';
	import Tv from '@lucide/svelte/icons/tv';
	import { onMount } from 'svelte';

	let devices = $state<Device[]>([]);
	let children = $state<Child[]>([]);
	let catalog = $state<Catalog | null>(null);
	let loaded = $state(false);

	// Ajout d'un ordinateur (agent)
	let agentOpen = $state(false);
	let agentName = $state('');
	let code = $state<null | { code: string; expires_at: number }>(null);
	let knownAgents = 0;

	// Ajout ou modification d'un appareil sans agent
	let netOpen = $state(false);
	let net = $state({ id: '', name: '', ip: '', child_id: '' });

	// Règles d'un appareil partagé
	let policyOpen = $state(false);
	let policyDevice = $state<Device | null>(null);
	let policyDraft = $state<Policy | null>(null);

	let removing = $state<string | null>(null);

	const agents = $derived(devices.filter((d) => d.kind === 'agent'));
	const network = $derived(devices.filter((d) => d.kind === 'network'));
	const origin = typeof location === 'undefined' ? '' : location.origin;

	async function load() {
		const [d, c] = await Promise.all([
			attempt(() => api.get<Device[]>('/devices')),
			attempt(() => api.get<Child[]>('/children'))
		]);
		if (d) devices = d;
		if (c) children = c;
		loaded = true;
		// L'ordinateur vient de s'enrôler : la feuille du code n'a plus lieu d'être.
		if (code && agents.length > knownAgents) {
			code = null;
			agentOpen = false;
		}
	}

	onMount(() => {
		load();
		loadCatalog().then((c) => (catalog = c)).catch(() => {});
		const timer = setInterval(load, 5000);
		return () => clearInterval(timer);
	});

	async function createCode(event: SubmitEvent) {
		event.preventDefault();
		knownAgents = agents.length;
		code = (await attempt(() => api.post<{ code: string; expires_at: number }>('/devices/enroll-code', { name: agentName }))) ?? null;
	}

	function openAgent() {
		agentName = '';
		code = null;
		agentOpen = true;
	}

	function openNet(device?: Device) {
		net = device
			? { id: device.id, name: device.name, ip: device.ip ?? '', child_id: device.child_id ?? '' }
			: { id: '', name: '', ip: '', child_id: '' };
		netOpen = true;
	}

	async function saveNet(event: SubmitEvent) {
		event.preventDefault();
		const existing = devices.find((d) => d.id === net.id);
		const body = {
			name: net.name,
			ip: net.ip,
			child_id: net.child_id || null,
			// Conserve les règles propres d'un appareil qui reste partagé.
			policy: net.child_id ? null : (existing?.policy ?? null)
		};
		const done = net.id
			? await attempt(() => api.put(`/devices/${net.id}`, body), 'Appareil modifié')
			: await attempt(() => api.post('/devices', body), 'Appareil ajouté');
		if (done) {
			netOpen = false;
			load();
		}
	}

	async function setAccount(device: Device, account: string, childId: string) {
		const accounts: Record<string, string | null> = { ...device.accounts, [account]: childId || null };
		if (await attempt(() => api.put(`/devices/${device.id}/accounts`, accounts), 'Compte mis à jour')) load();
	}

	function openPolicy(device: Device) {
		policyDevice = device;
		policyDraft = structuredClone($state.snapshot(device.policy));
		policyOpen = true;
	}

	async function savePolicy() {
		if (!policyDevice || !policyDraft) return;
		const body = { name: policyDevice.name, ip: policyDevice.ip, child_id: null, policy: $state.snapshot(policyDraft) };
		if (await attempt(() => api.put(`/devices/${policyDevice!.id}`, body), 'Règles enregistrées')) {
			policyOpen = false;
			load();
		}
	}

	async function remove(device: Device) {
		if (await attempt(() => api.del(`/devices/${device.id}`), `${device.name} supprimé`)) {
			removing = null;
			load();
		}
	}

	const childName = (id: string | null) => children.find((c) => c.id === id)?.name;
</script>

{#snippet removal(device: Device)}
	{#if removing === device.id}
		<div class="grid grid-cols-2 gap-2">
			<button class="btn-danger btn-sm" onclick={() => remove(device)}>Supprimer</button>
			<button class="btn-quiet btn-sm" onclick={() => (removing = null)}>Annuler</button>
		</div>
	{:else}
		<button class="btn-ghost btn-sm text-muted" onclick={() => (removing = device.id)}>Supprimer</button>
	{/if}
{/snippet}

<div class="space-y-10">
	<section>
		<div class="mb-1 flex items-center justify-between gap-3">
			<h1>Ordinateurs</h1>
			<button class="btn-quiet btn-sm bg-surface" onclick={openAgent}><Plus size={18} /> Ajouter</button>
		</div>
		<p class="muted mb-4 max-w-xl">
			L'agent installé sur l'ordinateur filtre, compte le temps et verrouille la session. Il continue de protéger hors
			de la maison.
		</p>

		{#if loaded && agents.length === 0}
			<div class="panel py-9 text-center">
				<h2>Aucun ordinateur pour l'instant</h2>
				<p class="muted mx-auto mt-2 max-w-sm">Vous obtiendrez un code à saisir sur l'ordinateur à protéger.</p>
				<button class="btn-primary mt-5" onclick={openAgent}><Plus size={20} /> Ajouter un ordinateur</button>
			</div>
		{/if}

		<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
			{#each agents as device (device.id)}
				<article class="panel min-w-0">
					<div class="flex items-center gap-3">
						<span class="bg-bg flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl"><Laptop size={24} /></span>
						<div class="min-w-0 flex-1">
							<h2 class="text-[1.2rem]">{device.name}</h2>
							<p class="muted truncate text-sm">{device.hostname}</p>
						</div>
						{#if device.online}
							<span class="tag-mint shrink-0">Connecté</span>
						{:else}
							<span class="tag-quiet shrink-0" title="Les dernières règles reçues restent appliquées">Vu {ago(device.last_seen)}</span>
						{/if}
					</div>

					<h3 class="mt-5 mb-1">Qui utilise quel compte ?</h3>
					{#if device.os_accounts.length === 0}
						<p class="muted text-sm">Aucun compte détecté pour l'instant.</p>
					{/if}
					<div class="space-y-3">
						{#each device.os_accounts as account (account)}
							<label class="block">
								<span class="label flex items-center gap-2">
									{account}
									{#if device.active_accounts.includes(account)}<span class="tag-sun">en cours</span>{/if}
								</span>
								<select class="field" value={device.accounts[account] ?? ''} onchange={(e) => setAccount(device, account, e.currentTarget.value)}>
									<option value="">Non filtré, compte d'un parent</option>
									{#each children as child (child.id)}
										<option value={child.id}>{child.name}</option>
									{/each}
								</select>
							</label>
						{/each}
					</div>
					{#if device.os_accounts.length > 0 && Object.keys(device.accounts).length === 0}
						<p class="bg-sun-soft mt-3 rounded-2xl px-4 py-3 text-sm">
							Aucun compte n'est rattaché à un enfant : cet ordinateur n'est pas encore filtré.
						</p>
					{/if}

					<div class="border-line mt-4 flex items-center justify-between gap-3 border-t pt-3">
						<span class="muted text-sm">Agent {device.agent_version}</span>
						{@render removal(device)}
					</div>
				</article>
			{/each}
		</div>
	</section>

	<section>
		<div class="mb-1 flex items-center justify-between gap-3">
			<h1>Sans agent</h1>
			<button class="btn-quiet btn-sm bg-surface" onclick={() => openNet()}><Plus size={18} /> Ajouter</button>
		</div>
		<p class="muted mb-4 max-w-xl">
			Télévision, console, tablette : filtrés par le DNS de Cotutelle, reconnus à leur adresse. Le temps d'écran n'y
			est pas compté.
		</p>

		{#if network.length > 0}
			<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
				{#each network as device (device.id)}
					<article class="panel min-w-0">
						<div class="flex items-center gap-3">
							<span class="bg-bg flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl"><Tv size={24} /></span>
							<div class="min-w-0 flex-1">
								<h2 class="text-[1.2rem]">{device.name}</h2>
								<p class="muted text-sm tabular-nums">{device.ip}</p>
							</div>
							<span class="tag-quiet">{device.child_id ? `Règles de ${childName(device.child_id) ?? '…'}` : 'Partagé'}</span>
						</div>
						<div class="border-line mt-4 flex flex-wrap items-center justify-between gap-2 border-t pt-3">
							<div class="flex gap-2">
								{#if !device.child_id}
									<button class="btn-quiet btn-sm" onclick={() => openPolicy(device)}>Règles</button>
								{/if}
								<button class="btn-quiet btn-sm" onclick={() => openNet(device)}>Modifier</button>
							</div>
							{@render removal(device)}
						</div>
					</article>
				{/each}
			</div>
		{/if}

		<details class="panel group mt-4">
			<summary class="flex list-none items-center justify-between gap-3 font-semibold [&::-webkit-details-marker]:hidden">
				Faire passer ces appareils par Cotutelle
				<Plus size={20} class="text-muted transition-transform group-open:rotate-45" />
			</summary>
			<div class="muted mt-3 max-w-xl space-y-3 text-[0.95rem]">
				<p>Ces appareils doivent utiliser le serveur Cotutelle comme serveur DNS. Deux façons de faire :</p>
				<p>
					<strong class="text-ink">Sur la box.</strong> Dans les réglages DHCP, indiquez l'adresse du serveur Cotutelle
					comme serveur DNS. Tout le réseau l'utilisera, et seuls les appareils déclarés ici seront filtrés.
				</p>
				<p>
					<strong class="text-ink">Sur l'appareil.</strong> Dans ses réglages réseau, choisissez un DNS manuel et
					saisissez l'adresse du serveur.
				</p>
				<p>Donnez une adresse fixe à chaque appareil déclaré, sinon le filtrage se perd quand elle change.</p>
			</div>
		</details>
	</section>
</div>

<Sheet bind:open={agentOpen} title="Ajouter un ordinateur">
	{#if !code}
		<form class="space-y-4" onsubmit={createCode}>
			<div>
				<label class="label" for="agent-name">Nom de l'ordinateur</label>
				<input id="agent-name" class="field" bind:value={agentName} placeholder="Portable de Léo" required maxlength="40" />
			</div>
			<button class="btn-primary btn-block">Obtenir un code</button>
		</form>
	{:else}
		<div class="space-y-5">
			<div class="bg-sun text-night rounded-3xl p-5 text-center">
				<p class="text-sm font-semibold">Code valable jusqu'à {clock(code.expires_at)}</p>
				<p class="display mt-1 text-[2.6rem] tracking-[0.08em]">{code.code}</p>
			</div>
			<ol class="list-decimal space-y-3 pl-5 text-[0.95rem]">
				<li>Sur l'ordinateur à protéger, installez le paquet <code class="font-semibold">cotutelle-agent</code>.</li>
				<li>
					Dans un terminal, avec un compte administrateur :
					<pre class="bg-night mt-2 overflow-x-auto rounded-2xl p-4 text-[0.8rem] leading-relaxed text-white">sudo cotutelle-agent enroll \
  --server {origin} \
  --code {code.code}
sudo systemctl enable --now cotutelle-agent</pre>
				</li>
			</ol>
			<p class="muted flex items-center gap-2 text-sm">
				<span class="bg-sun h-2.5 w-2.5 animate-pulse rounded-full"></span>
				En attente de l'ordinateur. Cette feuille se fermera toute seule.
			</p>
		</div>
	{/if}
</Sheet>

<Sheet bind:open={netOpen} title={net.id ? "Modifier l'appareil" : 'Ajouter un appareil'}>
	<form class="space-y-4" onsubmit={saveNet}>
		<div>
			<label class="label" for="net-name">Nom</label>
			<input id="net-name" class="field" bind:value={net.name} placeholder="TV du salon" required maxlength="40" />
		</div>
		<div>
			<label class="label" for="net-ip">Adresse IP sur le réseau</label>
			<input id="net-ip" class="field tabular-nums" bind:value={net.ip} placeholder="192.168.1.50" inputmode="decimal" required />
		</div>
		<div>
			<label class="label" for="net-child">Règles appliquées</label>
			<select id="net-child" class="field" bind:value={net.child_id}>
				<option value="">Appareil partagé, règles les plus strictes</option>
				{#each children as child (child.id)}
					<option value={child.id}>Celles de {child.name}</option>
				{/each}
			</select>
			<p class="hint">Avec les règles d'un enfant, ses horaires et ses ouvertures s'appliquent aussi.</p>
		</div>
		<button class="btn-primary btn-block">{net.id ? 'Enregistrer' : 'Ajouter'}</button>
	</form>
</Sheet>

<Sheet bind:open={policyOpen} title="Règles de {policyDevice?.name ?? ''}" wide>
	{#if policyDraft && catalog}
		<div class="space-y-8">
			<FilterEditor bind:filter={policyDraft.filter} {catalog} />
			<section>
				<h2>Horaires</h2>
				<p class="muted mb-2 text-sm">En dehors de ces plages, l'appareil n'a plus accès à Internet.</p>
				<ScheduleEditor bind:schedule={policyDraft.schedule} />
			</section>
			<button class="btn-primary btn-block" disabled={!scheduleValid(policyDraft.schedule)} onclick={savePolicy}>Enregistrer</button>
		</div>
	{/if}
</Sheet>
