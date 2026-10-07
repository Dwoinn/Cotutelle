<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import About from '#lib/components/About.svelte';
	import Logo from '#lib/components/Logo.svelte';

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

<div class="mx-auto flex min-h-dvh max-w-sm flex-col justify-center px-5 py-10">
	<div class="mb-8"><Logo size={56} /></div>
	<h1 class="text-[2.4rem]">Bienvenue.</h1>
	<p class="muted mt-1 mb-7">
		Créez le premier compte parent. Vous pourrez en ajouter un second ensuite, avec les mêmes droits.
	</p>
	<form class="space-y-4" onsubmit={submit}>
		<div>
			<label class="label" for="name">Votre nom</label>
			<input id="name" class="field bg-surface" bind:value={name} placeholder="Camille" autocomplete="username" required />
		</div>
		<div>
			<label class="label" for="password">Mot de passe</label>
			<input id="password" type="password" class="field bg-surface" bind:value={password} minlength="8" autocomplete="new-password" required />
			<p class="hint">8 caractères au moins. Choisissez-en un que vos enfants ne devineront pas.</p>
		</div>
		<div>
			<label class="label" for="confirm">Encore une fois</label>
			<input id="confirm" type="password" class="field bg-surface {mismatch ? 'border-cherry' : ''}" bind:value={confirm} autocomplete="new-password" required />
			{#if mismatch}<p class="text-cherry mt-1 text-sm font-semibold">Les deux mots de passe diffèrent.</p>{/if}
		</div>
		<button class="btn-primary btn-block" disabled={busy || mismatch}>Créer mon compte</button>
	</form>
	<About />
</div>
