// Services nommés : ce qu'une politique en fait, et le rattachement d'un
// site saisi à la main au service dont il fait partie.

import type { Policy, Service } from './types.ts';

/** Autorisé, bloqué mais proposé à l'enfant, ou bloqué sans être proposé. */
export type ServiceMode = 'open' | 'ask' | 'blocked';

export function serviceMode(filter: Policy['filter'], id: string): ServiceMode {
	if (!filter.blocked_services.includes(id)) return 'open';
	return filter.requestable_services.includes(id) ? 'ask' : 'blocked';
}

export function setServiceMode(filter: Policy['filter'], id: string, mode: ServiceMode) {
	// Listes triées comme côté serveur : revenir à l'état d'origine ne laisse
	// pas de modification à enregistrer.
	const set = (list: string[], present: boolean) => {
		const others = list.filter((s) => s !== id);
		return present ? [...others, id].sort() : others;
	};
	filter.blocked_services = set(filter.blocked_services, mode !== 'open');
	filter.requestable_services = set(filter.requestable_services, mode === 'ask');
}

/** Le nom de domaine d'une saisie libre : sans protocole, chemin ni « www. ». */
export function cleanSite(input: string): string {
	return input
		.trim()
		.toLowerCase()
		.replace(/^https?:\/\//, '')
		.split(/[/?#]/)[0]
		.replace(/\.$/, '')
		.replace(/^www\./, '');
}

/**
 * Le service dont un site fait partie, s'il y en a un : le serveur ouvrira
 * alors le service entier. Même règle que lui, le plus précis l'emporte.
 */
export function serviceForSite(services: Service[], input: string): Service | null {
	const site = cleanSite(input);
	let best: Service | null = null;
	let precision = 0;
	for (const service of services) {
		for (const domain of service.domains) {
			if ((site === domain || site.endsWith(`.${domain}`)) && domain.length > precision) {
				best = service;
				precision = domain.length;
			}
		}
	}
	return best;
}

/** « 10 sites », « 1 site ». */
export const siteCount = (domains: string[]) => `${domains.length} ${domains.length > 1 ? 'sites' : 'site'}`;
