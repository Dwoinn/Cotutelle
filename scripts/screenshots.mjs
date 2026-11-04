#!/usr/bin/env node
// Régénère les captures d'écran du README dans docs/assets/screenshots.
//
// Prérequis : une instance de développement remplie par scripts/dev-seed.sh,
// Google Chrome ou Chromium dans le PATH, Node 22 ou plus récent (WebSocket
// intégré). Chrome est piloté en mode headless par le protocole DevTools,
// sans dépendance à installer.
//
// Usage : scripts/screenshots.mjs [adresse du serveur] [dossier de sortie]
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';

const SERVER = process.argv[2] ?? 'http://127.0.0.1:8087';
const OUT = process.argv[3] ?? 'docs/assets/screenshots';
const CHROME = process.env.CHROME ?? 'google-chrome';
const PORT = 9333;
mkdirSync(OUT, { recursive: true });

// --- Connexion avec le compte parent de dev-seed.sh ------------------------
const login = await fetch(`${SERVER}/api/v1/auth/login`, {
	method: 'POST',
	headers: { 'content-type': 'application/json' },
	body: JSON.stringify({
		name: 'Parent',
		password: process.env.COTUTELLE_DEV_PASSWORD ?? 'cotutelle-dev'
	})
});
if (!login.ok) throw new Error(`connexion refusée (${login.status}) : l'instance est-elle remplie par dev-seed.sh ?`);
const cookie = login.headers.get('set-cookie').split(';')[0];
const [name, value] = cookie.split('=');
const children = await (await fetch(`${SERVER}/api/v1/children`, { headers: { cookie } })).json();
const leo = children.find((c) => c.name === 'Léo') ?? children[0];

// --- Chrome headless ------------------------------------------------------------
const profile = mkdtempSync(join(tmpdir(), 'cotutelle-shots-'));
const chrome = spawn(
	CHROME,
	[
		'--headless=new', '--no-sandbox', '--disable-gpu', '--hide-scrollbars',
		'--font-render-hinting=none', `--remote-debugging-port=${PORT}`,
		`--user-data-dir=${profile}`, 'about:blank'
	],
	{ stdio: 'ignore' }
);
process.on('exit', () => {
	chrome.kill();
	rmSync(profile, { recursive: true, force: true });
});
for (let i = 0; i < 50; i++) {
	try {
		await fetch(`http://127.0.0.1:${PORT}/json/version`);
		break;
	} catch {
		await sleep(200);
	}
}
const target = await (await fetch(`http://127.0.0.1:${PORT}/json/new?about:blank`, { method: 'PUT' })).json();
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve) => (ws.onopen = resolve));

let seq = 0;
const pending = new Map();
const listeners = new Map();
ws.onmessage = (event) => {
	const msg = JSON.parse(event.data);
	if (msg.id && pending.has(msg.id)) {
		const { resolve, reject } = pending.get(msg.id);
		pending.delete(msg.id);
		msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
	} else if (msg.method && listeners.has(msg.method)) {
		const waiting = listeners.get(msg.method);
		listeners.delete(msg.method);
		waiting.forEach((f) => f(msg.params));
	}
};
const send = (method, params = {}) =>
	new Promise((resolve, reject) => {
		const id = ++seq;
		pending.set(id, { resolve, reject });
		ws.send(JSON.stringify({ id, method, params }));
	});
const once = (method) => new Promise((resolve) => listeners.set(method, [...(listeners.get(method) ?? []), resolve]));

await send('Page.enable');
await send('Network.enable');
await send('Network.setCookie', { name, value, url: SERVER });

// --- Pages à capturer ---------------------------------------------------------
const tab = (label) => `[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(label)})?.click()`;
const DESKTOP = { width: 1280, height: 860, mobile: false };
const MOBILE = { width: 390, height: 844, mobile: true };
const SHOTS = [
	{ file: 'accueil', path: '/', viewport: DESKTOP },
	{ file: 'activite', path: `/enfants/${leo.id}`, viewport: DESKTOP },
	{ file: 'temps', path: `/enfants/${leo.id}`, viewport: MOBILE, action: tab('Temps') },
	{ file: 'filtre', path: `/enfants/${leo.id}/parametres`, viewport: MOBILE, action: tab('Filtre') },
	// L'espace enfant a sa propre palette, identique de jour et de nuit ;
	// le bandeau « Aperçu parent » est retiré pour montrer ce que l'enfant voit.
	{ file: 'espace-enfant', path: `/moi?apercu=${leo.id}`, viewport: MOBILE, schemes: ['light'], action: `document.querySelector('p.bg-sun')?.remove()` }
];

for (const shot of SHOTS) {
	for (const scheme of shot.schemes ?? ['light', 'dark']) {
		await send('Emulation.setDeviceMetricsOverride', { ...shot.viewport, deviceScaleFactor: 2 });
		await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: scheme }] });
		const loaded = once('Page.loadEventFired');
		await send('Page.navigate', { url: `${SERVER}${shot.path}` });
		await loaded;
		await sleep(2000); // chargement des données et fin des animations
		if (shot.action) {
			await send('Runtime.evaluate', { expression: shot.action });
			await sleep(1500);
		}
		const { data } = await send('Page.captureScreenshot', { format: 'png' });
		const suffix = (shot.schemes ?? []).length === 1 ? '' : scheme === 'dark' ? '-sombre' : '-clair';
		const file = join(OUT, `${shot.file}${suffix}.png`);
		writeFileSync(file, Buffer.from(data, 'base64'));
		console.log(file);
	}
}
ws.close();
chrome.kill(); // sinon le processus Chrome garde Node en vie
