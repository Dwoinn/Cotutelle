// Client HTTP de l'API. Toute erreur devient une ApiError au message lisible.

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string
	) {
		super(message);
	}
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
	let response: Response;
	try {
		response = await fetch(`/api/v1${path}`, {
			method,
			credentials: 'same-origin',
			headers: body === undefined ? undefined : { 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body)
		});
	} catch {
		throw new ApiError(0, 'Serveur injoignable');
	}
	const data = await response.json().catch(() => null);
	if (!response.ok) {
		throw new ApiError(response.status, data?.error ?? `Erreur ${response.status}`);
	}
	return data as T;
}

export const api = {
	get: <T>(path: string) => request<T>('GET', path),
	post: <T>(path: string, body: unknown = {}) => request<T>('POST', path, body),
	put: <T>(path: string, body: unknown) => request<T>('PUT', path, body),
	del: <T>(path: string) => request<T>('DELETE', path)
};
