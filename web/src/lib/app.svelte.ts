// État global de l'interface : session du parent, messages, catalogue.

import { ApiError, api } from './api';
import type { Catalog } from './types';

type Toast = { id: number; text: string; kind: 'ok' | 'error' };

export const app = $state({
	ready: false,
	setupDone: true,
	parent: null as null | { id: string; name: string },
	toasts: [] as Toast[]
});

let nextToast = 1;

export function toast(text: string, kind: Toast['kind'] = 'ok') {
	const id = nextToast++;
	app.toasts.push({ id, text, kind });
	setTimeout(() => {
		app.toasts = app.toasts.filter((t) => t.id !== id);
	}, kind === 'error' ? 6000 : 3000);
}

/**
 * Exécute un appel d'API ; en cas d'échec, affiche le message et rend `undefined`.
 */
export async function attempt<T>(action: () => Promise<T>, success?: string): Promise<T | undefined> {
	try {
		const result = await action();
		if (success) toast(success);
		return result;
	} catch (error) {
		if (error instanceof ApiError && error.status === 401) {
			app.parent = null;
		}
		toast(error instanceof Error ? error.message : 'Erreur inattendue', 'error');
		return undefined;
	}
}

let catalogCache: Promise<Catalog> | null = null;

export function catalog(): Promise<Catalog> {
	catalogCache ??= api.get<Catalog>('/catalog').catch((error) => {
		catalogCache = null;
		throw error;
	});
	return catalogCache;
}

export function resetCatalog() {
	catalogCache = null;
}
