<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import favicon from '#lib/assets/favicon.svg';
	import About from '#lib/components/About.svelte';

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

<div class="flex min-h-screen flex-col items-center justify-center p-4">
	<form class="card w-full max-w-sm space-y-4" onsubmit={submit}>
		<div class="text-center">
			<img src={favicon} alt="" class="mx-auto mb-3 h-14 w-14" />
			<h1>Cotutelle</h1>
			<p class="muted">Espace parents</p>
		</div>
		<div>
			<label class="label" for="name">Nom</label>
			<input id="name" class="input" bind:value={name} autocomplete="username" required />
		</div>
		<div>
			<label class="label" for="password">Mot de passe</label>
			<input id="password" type="password" class="input" bind:value={password} autocomplete="current-password" required />
		</div>
		<button class="btn-primary w-full" disabled={busy}>Se connecter</button>
	</form>
	<About />
</div>
