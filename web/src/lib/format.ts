// Mise en forme en français : durées, heures, jours, phrases d'état.

import { isAllDay, toMinutes } from './schedule.ts';
import type { AccessStatus, ClosedReason, Day, Grant, GrantTarget, Opening, Service, TimeRange } from './types.ts';

export const DAY_LABELS: Record<Day, string> = {
	monday: 'Lundi',
	tuesday: 'Mardi',
	wednesday: 'Mercredi',
	thursday: 'Jeudi',
	friday: 'Vendredi',
	saturday: 'Samedi',
	sunday: 'Dimanche'
};

const WEEKDAY_FROM_API: Record<string, string> = {
	Mon: 'lundi',
	Tue: 'mardi',
	Wed: 'mercredi',
	Thu: 'jeudi',
	Fri: 'vendredi',
	Sat: 'samedi',
	Sun: 'dimanche'
};

/** « 1 h 05 », « 45 min », « 0 min ». */
export function minutes(total: number): string {
	const m = Math.max(0, Math.round(total));
	const h = Math.floor(m / 60);
	// Espaces insécables : « 30 min » ne se coupe jamais en fin de ligne.
	if (h === 0) return `${m}\u00a0min`;
	return m % 60 === 0 ? `${h}\u00a0h` : `${h}\u00a0h\u00a0${String(m % 60).padStart(2, '0')}`;
}

export const seconds = (total: number) => minutes(Math.floor(total / 60));

/** « 17:00:00 » → « 17:00 » ; `null` → minuit. */
export const hm = (time: string | null) => (time === null ? '24:00' : time.slice(0, 5));

/** Heure parlée : « 20 h », « 17 h 30 », « minuit ». */
export function hour(time: string | null): string {
	if (time === null) return 'minuit';
	const [h, m] = time.split(':').map(Number);
	return m === 0 ? `${h}\u00a0h` : `${h}\u00a0h\u00a0${String(m).padStart(2, '0')}`;
}

export function range(r: TimeRange): string {
	return `${hour(r.start)} à ${hour(r.end)}`;
}

export function clock(ts: number): string {
	return new Date(ts * 1000).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
}

/** Minutes écoulées depuis minuit, heure locale. */
export function nowMinutes(date = new Date()): number {
	return date.getHours() * 60 + date.getMinutes();
}

/** « à l'instant », « il y a 5 min », « il y a 3 h », « le 3 oct. ». */
export function ago(ts: number | null, now = Date.now() / 1000): string {
	if (!ts) return 'jamais';
	const delta = now - ts;
	if (delta < 90) return "à l'instant";
	if (delta < 3600) return `il y a ${Math.round(delta / 60)} min`;
	if (delta < 86400) return `il y a ${Math.round(delta / 3600)} h`;
	return `le ${new Date(ts * 1000).toLocaleDateString('fr-FR', { day: 'numeric', month: 'short' })}`;
}

/** Temps restant avant une échéance : « 12 min », « 1 h 30 ». */
export function until(ts: number, now = Date.now() / 1000): string {
	return minutes(Math.max(0, (ts - now) / 60));
}

export function opening(next: Opening): string | null {
	if (!next) return null;
	const today = new Date().toLocaleDateString('en-US', { weekday: 'short' });
	const [h, m] = next.time.split(':').map(Number);
	const at = m === 0 ? `${h}\u00a0h` : `${h}\u00a0h\u00a0${String(m).padStart(2, '0')}`;
	return next.day === today ? `à ${at}` : `${WEEKDAY_FROM_API[next.day] ?? next.day} à ${at}`;
}

export const CLOSED_LABEL: Record<ClosedReason, string> = {
	paused: 'En pause',
	outside_schedule: 'Hors horaires',
	daily_quota: 'Temps du jour écoulé',
	weekly_quota: 'Temps de la semaine écoulé'
};

/** La ligne sous le cadran : jusqu'à quand, ou pourquoi c'est fermé. */
export function timeLine(status: AccessStatus, ranges: TimeRange[], next: Opening, now = nowMinutes()): string {
	if (!status.open) {
		if (status.reason === 'outside_schedule') {
			const when = opening(next);
			return when ? `Reprise ${when}` : CLOSED_LABEL.outside_schedule;
		}
		return status.reason ? CLOSED_LABEL[status.reason] : 'Fermé';
	}
	if (ranges.length === 0 || isAllDay(ranges)) return 'Toute la journée';
	const current = ranges.find((r) => toMinutes(r.start) <= now && now < toMinutes(r.end));
	return current ? `Jusqu'à ${hour(current.end)}` : 'Ouvert exceptionnellement';
}

export function targetLabel(target: GrantTarget, services: Pick<Service, 'id' | 'label'>[] = []): string {
	switch (target.kind) {
		case 'service':
			return services.find((s) => s.id === target.service)?.label ?? target.service;
		case 'category':
			return `Catégorie ${target.category}`;
		case 'domain':
			return target.domain;
		case 'extra_minutes':
			return `${minutes(target.minutes)} de plus`;
		case 'ignore_schedule':
			return 'Ouvert hors horaires';
		case 'pause':
			return 'Pause';
	}
}

/** « encore 44 min » ou « aujourd'hui » pour un bonus de temps. */
export function grantRemaining(grant: Grant, now: number): string {
	return grant.target.kind === 'extra_minutes' ? "aujourd'hui" : `encore ${until(grant.expires_at, now)}`;
}

const TIME_KINDS = ['extra_minutes', 'ignore_schedule', 'pause'];

/** Les mesures qui touchent au temps (cadran) ou au filtrage (bouclier). */
export const timeGrants = (grants: Grant[]) => grants.filter((g) => TIME_KINDS.includes(g.target.kind));
export const filterGrants = (grants: Grant[]) => grants.filter((g) => !TIME_KINDS.includes(g.target.kind));

/** « 2026-10-07 » → « mer. 7 ». */
export function shortDay(day: string): string {
	const date = new Date(`${day}T12:00:00`);
	return date.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric' });
}

/** Initiale d'un prénom, pour le monogramme. */
export function initial(name: string): string {
	return (name.trim()[0] ?? '?').toUpperCase();
}
