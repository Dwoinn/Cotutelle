<script lang="ts">
	import '../app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '#lib/api.ts';
	import { app, attempt } from '#lib/app.svelte.ts';
	import favicon from '#lib/assets/favicon.svg';
	import Logo from '#lib/components/Logo.svelte';
	import Toasts from '#lib/components/Toasts.svelte';
	import House from '@lucide/svelte/icons/house';
	import Laptop from '@lucide/svelte/icons/laptop';
	import LogOut from '@lucide/svelte/icons/log-out';
	import Settings from '@lucide/svelte/icons/settings';
	import { onMount } from 'svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const path = $derived(page.url.pathname);
	// L'espace enfant a sa propre session et sa propre mise en page.
	const childSpace = $derived(path.startsWith('/moi'));
	const publicPage = $derived(path === '/connexion' || path === '/installation' || childSpace);

	const NAV = [
		{ href: '/', label: 'Accueil', icon: House },
		{ href: '/appareils', label: 'Appareils', icon: Laptop },
		{ href: '/reglages', label: 'Réglages', icon: Settings }
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
	<div class="flex min-h-dvh items-center justify-center"><Logo size={48} /></div>
{:else if childSpace || publicPage}
	{@render children()}
{:else if app.parent}
	<div class="mx-auto min-h-dvh max-w-5xl px-4 pb-32 sm:px-6 sm:pb-16">
		<header class="flex items-center justify-between gap-4 py-4 sm:py-6">
			<a href="/" aria-label="Accueil Cotutelle"><Logo size={34} wordmark /></a>
			<nav class="hidden items-center gap-1 sm:flex" aria-label="Navigation principale">
				{#each NAV as item (item.href)}
					<a href={item.href} class="btn btn-sm {active(item.href) ? 'bg-surface' : 'text-muted hover:text-ink'}" aria-current={active(item.href) ? 'page' : undefined}>
						{item.label}
					</a>
				{/each}
			</nav>
			<button class="icon-btn" onclick={logout} aria-label="Se déconnecter ({app.parent.name})" title="Se déconnecter"><LogOut size={21} /></button>
		</header>
		<main>{@render children()}</main>
	</div>

	<!-- Navigation au pouce sur téléphone -->
	<nav
		class="bg-surface border-line fixed inset-x-0 bottom-0 z-30 flex border-t px-2 pt-1.5 pb-[max(0.5rem,env(safe-area-inset-bottom))] sm:hidden"
		aria-label="Navigation principale"
	>
		{#each NAV as item (item.href)}
			{@const Icon = item.icon}
			<a
				href={item.href}
				class="flex min-h-14 flex-1 flex-col items-center justify-center gap-0.5 rounded-2xl text-xs font-semibold {active(item.href) ? 'text-ink' : 'text-muted'}"
				aria-current={active(item.href) ? 'page' : undefined}
			>
				<span class="flex h-8 w-14 items-center justify-center rounded-full {active(item.href) ? 'bg-sun text-night' : ''}">
					<Icon size={22} strokeWidth={active(item.href) ? 2.4 : 2} />
				</span>
				{item.label}
			</a>
		{/each}
	</nav>
{/if}

<Toasts />
