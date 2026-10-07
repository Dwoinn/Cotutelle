// Outils pour les plages horaires.

import { DAYS, type Day, type Schedule, type TimeRange } from './types';

/** Minutes depuis minuit ; `null` (fin de journée) vaut 1440. */
export function toMinutes(time: string | null): number {
	if (time === null) return 24 * 60;
	const [h, m] = time.split(':').map(Number);
	return h * 60 + m;
}

export const rangeValid = (r: TimeRange) => toMinutes(r.start) < toMinutes(r.end);

export function scheduleValid(schedule: Schedule): boolean {
	return DAYS.every((day) => schedule[day].every(rangeValid));
}

export function totalMinutes(ranges: TimeRange[]): number {
	return ranges.filter(rangeValid).reduce((sum, r) => sum + toMinutes(r.end) - toMinutes(r.start), 0);
}

export const SCHOOL_DAYS: Day[] = ['monday', 'tuesday', 'thursday', 'friday'];
export const FREE_DAYS: Day[] = ['wednesday', 'saturday', 'sunday'];

export const ALL_DAY: TimeRange = { start: '00:00:00', end: null };

export const isAllDay = (ranges: TimeRange[]) =>
	ranges.length === 1 && toMinutes(ranges[0].start) === 0 && ranges[0].end === null;
