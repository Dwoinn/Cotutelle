import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// Adresse du serveur Cotutelle pendant le développement de l'interface.
const api = process.env.COTUTELLE_API ?? 'http://127.0.0.1:8080';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Application monopage servie en statique par cotutelle-server.
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	server: { proxy: { '/api': api } }
});
