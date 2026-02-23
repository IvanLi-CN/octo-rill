export class ApiError extends Error {
	public status: number;
	public code: string;

	constructor(status: number, message: string, code = "unknown_error") {
		super(message);
		this.status = status;
		this.code = code;
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

function toApiError(res: Response, body: unknown) {
	if (typeof body === "object" && body && "error" in body) {
		const payload = body as {
			error?: { code?: string; message?: string };
		};
		return new ApiError(
			res.status,
			String(payload.error?.message ?? res.statusText),
			String(payload.error?.code ?? "unknown_error"),
		);
	}
	return new ApiError(res.status, res.statusText);
}

export async function apiGet<T>(path: string): Promise<T> {
	const res = await fetch(path, { credentials: "include" });
	if (!res.ok) {
		const body = await parseJson(res);
		throw toApiError(res, body);
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
		throw toApiError(res, await parseJson(res));
	}
	return (await res.json()) as T;
}

export async function apiPutJson<T>(path: string, body?: unknown): Promise<T> {
	const res = await fetch(path, {
		method: "PUT",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	if (!res.ok) {
		throw toApiError(res, await parseJson(res));
	}
	return (await res.json()) as T;
}
