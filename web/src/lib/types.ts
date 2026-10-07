// Types de l'API Cotutelle, miroir des structures Rust de `cotutelle-common`.

export type TimeRange = { start: string; end: string | null };

export const DAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday'] as const;
export type Day = (typeof DAYS)[number];
export type Schedule = Record<Day, TimeRange[]>;

export type Policy = {
	filter: {
		blocked_categories: string[];
		blocked_services: string[];
		allow: string[];
		deny: string[];
		youtube_restricted: boolean;
		allow_requests: boolean;
	};
	schedule: Schedule;
	quota: { daily_minutes: number | null; weekly_minutes: number | null };
};

export type GrantTarget =
	| { kind: 'service'; service: string }
	| { kind: 'category'; category: string }
	| { kind: 'domain'; domain: string }
	| { kind: 'extra_minutes'; minutes: number }
	| { kind: 'ignore_schedule' }
	| { kind: 'pause' };

export type Grant = {
	id: string;
	child_id: string | null;
	device_id: string | null;
	target: GrantTarget;
	starts_at: number;
	expires_at: number;
	granted_by: string | null;
};

export type ClosedReason = 'paused' | 'outside_schedule' | 'daily_quota' | 'weekly_quota';

export type AccessStatus = {
	open: boolean;
	reason: ClosedReason | null;
	remaining_minutes: number | null;
	quota_today_minutes: number | null;
	used_today_minutes: number;
};

export type Opening = { day: string; time: string } | null;

export type Child = {
	id: string;
	name: string;
	birth_year: number | null;
	emoji: string;
	color: string;
	policy: Policy;
};

export type Device = {
	id: string;
	name: string;
	kind: 'agent' | 'network';
	hostname: string | null;
	os: string | null;
	agent_version: string | null;
	ip: string | null;
	child_id: string | null;
	policy: Policy | null;
	os_accounts: string[];
	accounts: Record<string, string>;
	last_seen: number | null;
	online: boolean;
	active_accounts: string[];
};

export type ChildRequest = {
	id: string;
	child_id: string;
	child_name: string;
	label: string | null;
	target: GrantTarget | null;
	minutes: number | null;
	message: string | null;
	status: 'pending' | 'approved' | 'denied';
	created_at: number;
};

export type Alert = { id: string; kind: string; message: string; created_at: number };

export type ChildSummary = {
	id: string;
	name: string;
	emoji: string;
	color: string;
	status: AccessStatus;
	used_today_seconds: number;
	used_week_seconds: number;
	next_opening: Opening;
	grants: Grant[];
	blocked_services: string[];
	devices: { id: string; name: string; kind: string; online: boolean; active: boolean }[];
};

export type Dashboard = {
	children: ChildSummary[];
	devices: Device[];
	device_grants: Grant[];
	requests: ChildRequest[];
	alerts: Alert[];
	blocklists: { updated_at: number; categories: number };
	now: number;
};

export type Service = { id: string; label: string; icon: string; domains: string[] };

export type Catalog = {
	groups: {
		id: string;
		label: string;
		categories: { id: string; label: string; description: string; recommended: boolean; entries: number | null }[];
	}[];
	services: Service[];
	blocklists_ready: boolean;
};

export type Activity = {
	days: { day: string; seconds: number }[];
	devices: { id: string; name: string; seconds: number }[];
	top_domains: { domain: string; allowed: number; blocked: number }[];
	blocked: { domain: string; blocked: number; reason: { kind: string; id?: string } | null }[];
};

export type ChildSpace = {
	child: { id: string; name: string; emoji: string; color: string };
	status: AccessStatus;
	used_today_seconds: number;
	next_opening: Opening;
	today_ranges: TimeRange[];
	grants: Grant[];
	blocked_services: { id: string; label: string; icon: string }[];
	allow_requests: boolean;
	requests: ChildRequest[];
	activity: Activity;
	now: number;
};

export type Settings = {
	ntfy_url: string;
	upstream_dns: string;
	retention_days: number;
	blocklists: { updated_at: number; source: string; categories: number; entries: number };
	version: string;
};
