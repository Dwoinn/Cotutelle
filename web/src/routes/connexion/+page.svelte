<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import About from '#lib/components/About.svelte';
	import Logo from '#lib/components/Logo.svelte';

	let name = $state('');
	let password = $state('');
	let busy = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		const done = await attempt(() => api.post('/auth/login', { name, password }));
		if (done) {
			app.parent = await api.get<{ id: string; name: string }>('/auth/me').catch(() => null);
		}
		busy = false;
	}
</script>

<div class="mx-auto flex min-h-dvh max-w-sm flex-col justify-center px-5 py-10">
	<div class="mb-8"><Logo size={56} /></div>
	<h1 class="text-[2.4rem]">Bonjour.</h1>
	<p class="muted mt-1 mb-7">Connectez-vous à l'espace parents.</p>
	<form class="space-y-4" onsubmit={submit}>
		<div>
			<label class="label" for="name">Nom</label>
			<input id="name" class="field bg-surface" bind:value={name} autocomplete="username" autocapitalize="off" required />
		</div>
		<div>
			<label class="label" for="password">Mot de passe</label>
			<input id="password" type="password" class="field bg-surface" bind:value={password} autocomplete="current-password" required />
		</div>
		<button class="btn-primary btn-block" disabled={busy}>Se connecter</button>
	</form>
	<About />
</div>
