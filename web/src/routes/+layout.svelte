<script lang="ts">
	import '../app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import favicon from '#lib/assets/favicon.svg';
	import Toasts from '#lib/components/Toasts.svelte';
	import { onMount } from 'svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const path = $derived(page.url.pathname);
	// L'espace enfant a sa propre session et sa propre mise en page.
	const childSpace = $derived(path.startsWith('/moi'));
	const publicPage = $derived(path === '/connexion' || path === '/installation' || childSpace);

	const NAV = [
		{ href: '/', label: 'Accueil', icon: '🏠' },
		{ href: '/appareils', label: 'Appareils', icon: '💻' },
		{ href: '/reglages', label: 'Réglages', icon: '⚙️' }
	];

	onMount(async () => {
		try {
			const status = await api.get<{ setup_done: boolean }>('/status');
			app.setupDone = status.setup_done;
			if (status.setup_done) {
				app.parent = await api.get<{ id: string; name: string }>('/auth/me').catch(() => null);
			}
		} catch {
			// Serveur injoignable : les pages afficheront leur propre erreur.
		}
		app.ready = true;
	});

	// Garde de navigation : installation d'abord, connexion ensuite.
	$effect(() => {
		if (!app.ready || childSpace) return;
		if (!app.setupDone) {
			if (path !== '/installation') goto('/installation', { replaceState: true });
		} else if (!app.parent) {
			if (path !== '/connexion') goto('/connexion', { replaceState: true });
		} else if (publicPage) {
			goto('/', { replaceState: true });
		}
	});

	async function logout() {
		await attempt(() => api.post('/auth/logout'));
		app.parent = null;
	}

	const active = (href: string) => (href === '/' ? path === '/' || path.startsWith('/enfants') : path.startsWith(href));
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if !app.ready}
	<div class="muted flex min-h-screen items-center justify-center">Chargement…</div>
{:else if childSpace || publicPage}
	{@render children()}
{:else if app.parent}
	<div class="mx-auto min-h-screen max-w-5xl px-4 pb-28 sm:pb-12">
		<header class="flex items-center justify-between gap-4 py-5">
			<a href="/" class="flex items-center gap-2.5">
				<img src={favicon} alt="" class="h-9 w-9" />
				<span class="text-xl font-bold tracking-tight">Cotutelle</span>
			</a>
			<nav class="hidden items-center gap-1 sm:flex">
				{#each NAV as item (item.href)}
					<a href={item.href} class={active(item.href) ? 'btn-soft' : 'btn-ghost'}>{item.label}</a>
				{/each}
			</nav>
			<div class="flex items-center gap-2">
				<span class="muted hidden sm:inline">{app.parent.name}</span>
				<button class="btn-ghost btn-sm" onclick={logout}>Déconnexion</button>
			</div>
		</header>
		<main>{@render children()}</main>
	</div>

	<!-- Navigation basse sur téléphone -->
	<nav
		class="fixed inset-x-0 bottom-0 z-30 flex justify-around border-t border-slate-200 bg-white/95 px-2 pt-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] backdrop-blur sm:hidden dark:border-slate-800 dark:bg-slate-900/95"
	>
		{#each NAV as item (item.href)}
			<a
				href={item.href}
				class="flex flex-1 flex-col items-center gap-0.5 rounded-2xl py-1.5 text-xs font-medium {active(item.href)
					? 'text-brand-600 dark:text-brand-300'
					: 'text-slate-500'}"
			>
				<span class="text-xl">{item.icon}</span>
				{item.label}
			</a>
		{/each}
	</nav>
{/if}

<Toasts />
