<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import favicon from '#lib/assets/favicon.svg';

	let name = $state('');
	let password = $state('');
	let confirm = $state('');
	let busy = $state(false);

	const mismatch = $derived(confirm.length > 0 && confirm !== password);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (password !== confirm) return;
		busy = true;
		const done = await attempt(() => api.post('/setup', { name, password }));
		if (done) {
			app.setupDone = true;
			app.parent = await api.get<{ id: string; name: string }>('/auth/me').catch(() => null);
		}
		busy = false;
	}
</script>

<div class="flex min-h-screen items-center justify-center p-4">
	<form class="card w-full max-w-md space-y-4" onsubmit={submit}>
		<div class="text-center">
			<img src={favicon} alt="" class="mx-auto mb-3 h-14 w-14" />
			<h1>Bienvenue dans Cotutelle</h1>
			<p class="muted mt-1">
				Créez le premier compte parent. Vous pourrez en ajouter un second ensuite, avec les mêmes droits.
			</p>
		</div>
		<div>
			<label class="label" for="name">Votre nom</label>
			<input id="name" class="input" bind:value={name} placeholder="Papa, Maman, Camille…" autocomplete="username" required />
		</div>
		<div>
			<label class="label" for="password">Mot de passe</label>
			<input id="password" type="password" class="input" bind:value={password} minlength="8" autocomplete="new-password" required />
			<p class="muted mt-1 text-xs">8 caractères minimum. Choisissez-en un que vos enfants ne devineront pas.</p>
		</div>
		<div>
			<label class="label" for="confirm">Confirmation</label>
			<input id="confirm" type="password" class="input {mismatch ? 'border-rose-400' : ''}" bind:value={confirm} autocomplete="new-password" required />
			{#if mismatch}<p class="mt-1 text-xs text-rose-600">Les deux mots de passe diffèrent.</p>{/if}
		</div>
		<button class="btn-primary w-full" disabled={busy || mismatch}>Créer mon compte</button>
	</form>
</div>
