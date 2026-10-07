<script lang="ts">
	import { api } from '#lib/api.ts';
	import { attempt, catalog as loadCatalog } from '#lib/app.svelte.ts';
	import FilterEditor from '#lib/components/FilterEditor.svelte';
	import Modal from '#lib/components/Modal.svelte';
	import ScheduleEditor from '#lib/components/ScheduleEditor.svelte';
	import { ago, clock } from '#lib/format.ts';
	import { scheduleValid } from '#lib/schedule.ts';
	import type { Catalog, Child, Device, Policy } from '#lib/types.ts';
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

	// Ajout ou modification d'un appareil réseau
	let netOpen = $state(false);
	let net = $state({ id: '', name: '', ip: '', child_id: '' });

	// Modification de la politique d'un appareil partagé
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
		// L'ordinateur vient de s'enrôler : la fenêtre du code n'a plus lieu d'être.
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
			// Conserve la politique propre d'un appareil qui reste partagé.
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

<div class="space-y-8">
	<section>
		<div class="mb-1 flex items-center justify-between gap-3">
			<h1>Ordinateurs</h1>
			<button class="btn-soft" onclick={openAgent}>+ Ajouter</button>
		</div>
		<p class="muted mb-3">
			Un agent installé sur l'ordinateur filtre, compte le temps et verrouille la session. Il continue de protéger
			hors de la maison.
		</p>

		{#if loaded && agents.length === 0}
			<div class="card py-8 text-center">
				<p class="text-4xl">💻</p>
				<p class="mt-2 font-medium">Aucun ordinateur pour l'instant</p>
				<button class="btn-primary mt-3" onclick={openAgent}>Ajouter un ordinateur</button>
			</div>
		{/if}

		<div class="grid gap-4 md:grid-cols-2">
			{#each agents as device (device.id)}
				<article class="card space-y-3">
					<div class="flex items-start justify-between gap-3">
						<div>
							<p class="flex items-center gap-2 font-semibold"><span class="text-2xl">💻</span>{device.name}</p>
							<p class="muted text-xs">
								{device.hostname} · agent {device.agent_version}
							</p>
						</div>
						{#if device.online}
							<span class="pill-ok">Connecté</span>
						{:else}
							<span class="pill-off" title="La dernière politique connue reste appliquée">Vu {ago(device.last_seen)}</span>
						{/if}
					</div>

					<div>
						<p class="label">Qui utilise quel compte ?</p>
						{#if device.os_accounts.length === 0}
							<p class="muted text-xs">Aucun compte détecté pour l'instant.</p>
						{/if}
						<div class="space-y-2">
							{#each device.os_accounts as account (account)}
								<div class="flex items-center gap-3">
									<span class="flex-1 truncate font-mono text-sm">
										{account}
										{#if device.active_accounts.includes(account)}<span class="pill-ok ml-1">en cours</span>{/if}
									</span>
									<select
										class="input w-44"
										aria-label="Enfant pour le compte {account}"
										value={device.accounts[account] ?? ''}
										onchange={(e) => setAccount(device, account, e.currentTarget.value)}
									>
										<option value="">Non filtré (parent)</option>
										{#each children as child (child.id)}
											<option value={child.id}>{child.emoji} {child.name}</option>
										{/each}
									</select>
								</div>
							{/each}
						</div>
						{#if device.os_accounts.length > 0 && Object.keys(device.accounts).length === 0}
							<p class="pill-warn mt-2 rounded-2xl px-3 py-2 text-xs">
								Aucun compte n'est rattaché à un enfant : cet ordinateur n'est pas encore filtré.
							</p>
						{/if}
					</div>

					<div class="flex justify-end border-t border-slate-100 pt-3 dark:border-slate-800">
						{#if removing === device.id}
							<button class="btn-danger btn-sm" onclick={() => remove(device)}>Confirmer la suppression</button>
							<button class="btn-ghost btn-sm" onclick={() => (removing = null)}>Annuler</button>
						{:else}
							<button class="btn-ghost btn-sm" onclick={() => (removing = device.id)}>Supprimer</button>
						{/if}
					</div>
				</article>
			{/each}
		</div>
	</section>

	<section>
		<div class="mb-1 flex items-center justify-between gap-3">
			<h1>Appareils sans agent</h1>
			<button class="btn-soft" onclick={() => openNet()}>+ Ajouter</button>
		</div>
		<p class="muted mb-3">
			Télévision, console, tablette : ils sont filtrés par le DNS de Cotutelle, reconnu à leur adresse IP. Le temps
			d'écran n'y est pas compté.
		</p>

		<div class="grid gap-4 md:grid-cols-2">
			{#each network as device (device.id)}
				<article class="card space-y-3">
					<div class="flex items-start justify-between gap-3">
						<div>
							<p class="flex items-center gap-2 font-semibold"><span class="text-2xl">📺</span>{device.name}</p>
							<p class="muted font-mono text-xs">{device.ip}</p>
						</div>
						<span class="chip">{device.child_id ? `Règles de ${childName(device.child_id) ?? '…'}` : 'Partagé'}</span>
					</div>
					<div class="flex flex-wrap justify-end gap-2 border-t border-slate-100 pt-3 dark:border-slate-800">
						{#if !device.child_id}
							<button class="btn-soft btn-sm" onclick={() => openPolicy(device)}>Règles</button>
						{/if}
						<button class="btn-ghost btn-sm" onclick={() => openNet(device)}>Modifier</button>
						{#if removing === device.id}
							<button class="btn-danger btn-sm" onclick={() => remove(device)}>Confirmer</button>
							<button class="btn-ghost btn-sm" onclick={() => (removing = null)}>Annuler</button>
						{:else}
							<button class="btn-ghost btn-sm" onclick={() => (removing = device.id)}>Supprimer</button>
						{/if}
					</div>
				</article>
			{/each}
		</div>

		<details class="card mt-4">
			<summary class="font-semibold">Comment faire passer ces appareils par Cotutelle ?</summary>
			<div class="muted mt-3 space-y-2">
				<p>
					Les appareils doivent utiliser le serveur Cotutelle comme serveur DNS. Deux possibilités :
				</p>
				<ol class="list-decimal space-y-1 pl-5">
					<li>
						<strong>Sur la box</strong> : dans les réglages DHCP, indiquez l'adresse IP du serveur Cotutelle comme
						serveur DNS. Tous les appareils du réseau l'utiliseront ; seuls ceux déclarés ici sont filtrés.
					</li>
					<li>
						<strong>Sur l'appareil</strong> : dans ses réglages réseau, choisissez une configuration DNS manuelle
						et saisissez l'adresse IP du serveur.
					</li>
				</ol>
				<p>
					Donnez une adresse IP fixe à chaque appareil déclaré (bail DHCP statique sur la box), sinon le filtrage
					se perd quand l'adresse change.
				</p>
			</div>
		</details>
	</section>
</div>

<Modal bind:open={agentOpen} title="Ajouter un ordinateur">
	{#if !code}
		<form class="space-y-4" onsubmit={createCode}>
			<div>
				<label class="label" for="agent-name">Nom de l'ordinateur</label>
				<input id="agent-name" class="input" bind:value={agentName} placeholder="Portable de Louis" required maxlength="40" />
			</div>
			<button class="btn-primary w-full">Obtenir un code</button>
		</form>
	{:else}
		<div class="space-y-4">
			<div class="bg-brand-50 dark:bg-brand-900/30 rounded-2xl p-4 text-center">
				<p class="muted text-xs">Code d'enrôlement, valable jusqu'à {clock(code.expires_at)}</p>
				<p class="text-brand-700 dark:text-brand-200 mt-1 font-mono text-3xl font-bold tracking-widest">{code.code}</p>
			</div>
			<ol class="list-decimal space-y-2 pl-5 text-sm">
				<li>Sur l'ordinateur à protéger, installez le paquet <code>cotutelle-agent</code>.</li>
				<li>
					Dans un terminal, avec un compte administrateur :
					<pre class="mt-1 overflow-x-auto rounded-xl bg-slate-900 p-3 text-xs text-slate-100">sudo cotutelle-agent enroll \
  --server {origin} \
  --code {code.code}
sudo systemctl enable --now cotutelle-agent</pre>
				</li>
				<li>Cette fenêtre se fermera dès que l'ordinateur sera rattaché.</li>
			</ol>
			<p class="muted flex items-center gap-2 text-xs">
				<span class="bg-brand-500 h-2 w-2 animate-pulse rounded-full"></span> En attente de l'ordinateur…
			</p>
		</div>
	{/if}
</Modal>

<Modal bind:open={netOpen} title={net.id ? "Modifier l'appareil" : 'Ajouter un appareil sans agent'}>
	<form class="space-y-4" onsubmit={saveNet}>
		<div>
			<label class="label" for="net-name">Nom</label>
			<input id="net-name" class="input" bind:value={net.name} placeholder="TV du salon" required maxlength="40" />
		</div>
		<div>
			<label class="label" for="net-ip">Adresse IP sur le réseau local</label>
			<input id="net-ip" class="input font-mono" bind:value={net.ip} placeholder="192.168.1.50" required />
		</div>
		<div>
			<label class="label" for="net-child">Règles appliquées</label>
			<select id="net-child" class="input" bind:value={net.child_id}>
				<option value="">Appareil partagé : les règles les plus strictes de la maison</option>
				{#each children as child (child.id)}
					<option value={child.id}>Celles de {child.name} (horaires et exceptions compris)</option>
				{/each}
			</select>
		</div>
		<button class="btn-primary w-full">{net.id ? 'Enregistrer' : 'Ajouter'}</button>
	</form>
</Modal>

<Modal bind:open={policyOpen} title="Règles de {policyDevice?.name ?? ''}" wide>
	{#if policyDraft && catalog}
		<div class="space-y-6">
			<FilterEditor bind:filter={policyDraft.filter} {catalog} />
			<section>
				<h3 class="font-semibold">Horaires</h3>
				<p class="muted mb-3">En dehors de ces plages, l'appareil n'a plus accès à Internet.</p>
				<ScheduleEditor bind:schedule={policyDraft.schedule} />
			</section>
			<button class="btn-primary w-full" disabled={!scheduleValid(policyDraft.schedule)} onclick={savePolicy}>Enregistrer</button>
		</div>
	{/if}
</Modal>
