// Mise en forme en français : durées, heures, jours, cibles d'exception.

import type { ClosedReason, Day, GrantTarget, Opening, Service, TimeRange } from './types';

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
	if (h === 0) return `${m} min`;
	return m % 60 === 0 ? `${h} h` : `${h} h ${String(m % 60).padStart(2, '0')}`;
}

export const seconds = (total: number) => minutes(Math.floor(total / 60));

/** « 17:00:00 » → « 17:00 » ; `null` → minuit. */
export const hm = (time: string | null) => (time === null ? '24:00' : time.slice(0, 5));

export function range(r: TimeRange): string {
	return `${hm(r.start)} – ${hm(r.end)}`;
}

export function clock(ts: number): string {
	return new Date(ts * 1000).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
}

/** « à l'instant », « il y a 5 min », « hier à 18:02 », « le 3 oct. ». */
export function ago(ts: number | null, now = Date.now() / 1000): string {
	if (!ts) return 'jamais';
	const delta = now - ts;
	if (delta < 90) return "à l'instant";
	if (delta < 3600) return `il y a ${Math.round(delta / 60)} min`;
	if (delta < 86400) return `il y a ${Math.round(delta / 3600)} h`;
	return new Date(ts * 1000).toLocaleDateString('fr-FR', { day: 'numeric', month: 'short' });
}

/** Temps restant avant une échéance : « 12 min », « 1 h 30 ». */
export function until(ts: number, now = Date.now() / 1000): string {
	return minutes(Math.max(0, (ts - now) / 60));
}

export function opening(next: Opening): string | null {
	if (!next) return null;
	const today = new Date().toLocaleDateString('en-US', { weekday: 'short' });
	const day = next.day === today ? "aujourd'hui" : (WEEKDAY_FROM_API[next.day] ?? next.day);
	return `${day} à ${next.time}`;
}

export const CLOSED_LABEL: Record<ClosedReason, string> = {
	paused: 'En pause',
	outside_schedule: 'Hors horaires',
	daily_quota: 'Temps du jour écoulé',
	weekly_quota: 'Temps de la semaine écoulé'
};

export function targetLabel(target: GrantTarget, services: Service[] = []): string {
	switch (target.kind) {
		case 'service':
			return services.find((s) => s.id === target.service)?.label ?? target.service;
		case 'category':
			return `Catégorie ${target.category}`;
		case 'domain':
			return target.domain;
		case 'extra_minutes':
			return `+${minutes(target.minutes)}`;
		case 'ignore_schedule':
			return 'Hors horaires autorisé';
		case 'pause':
			return 'Pause';
	}
}

export function targetIcon(target: GrantTarget, services: Service[] = []): string {
	switch (target.kind) {
		case 'service':
			return services.find((s) => s.id === target.service)?.icon ?? '🔓';
		case 'extra_minutes':
			return '⏱️';
		case 'ignore_schedule':
			return '🌙';
		case 'pause':
			return '⏸️';
		default:
			return '🔓';
	}
}

/** « 2026-10-07 » → « mer. 7 ». */
export function shortDay(day: string): string {
	const date = new Date(`${day}T12:00:00`);
	return date.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric' });
}
