export class ApiError extends Error {
	public status: number;

	constructor(status: number, message: string) {
		super(message);
		this.status = status;
	}
}

async function parseJson(res: Response) {
	const text = await res.text();
	if (!text) return null;
	try {
		return JSON.parse(text) as unknown;
	} catch {
		return text;
	}
}

export async function apiGet<T>(path: string): Promise<T> {
	const res = await fetch(path, { credentials: "include" });
	if (!res.ok) {
		const body = await parseJson(res);
		const message =
			typeof body === "object" && body && "error" in body
				? // biome-ignore lint/suspicious/noExplicitAny: api error shape is dynamic
					String((body as any).error?.message ?? res.statusText)
				: res.statusText;
		throw new ApiError(res.status, message);
	}
	return (await res.json()) as T;
}

export async function apiPost<T>(path: string): Promise<T> {
	return apiPostJson<T>(path);
}

export async function apiPostJson<T>(path: string, body?: unknown): Promise<T> {
	const res = await fetch(path, {
		method: "POST",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	if (!res.ok) {
		const body = await parseJson(res);
		const message =
			typeof body === "object" && body && "error" in body
				? // biome-ignore lint/suspicious/noExplicitAny: api error shape is dynamic
					String((body as any).error?.message ?? res.statusText)
				: res.statusText;
		throw new ApiError(res.status, message);
	}
	return (await res.json()) as T;
}
