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

export type ReleaseDetailTranslated = {
	lang: string;
	status: "ready" | "missing" | "disabled";
	title: string | null;
	summary: string | null;
};

export type ReleaseDetailResponse = {
	release_id: string;
	repo_full_name: string | null;
	tag_name: string;
	name: string | null;
	body: string | null;
	html_url: string;
	published_at: string | null;
	is_prerelease: number;
	is_draft: number;
	translated: ReleaseDetailTranslated | null;
};

export async function apiGetReleaseDetail(
	releaseId: string,
): Promise<ReleaseDetailResponse> {
	return apiGet<ReleaseDetailResponse>(
		`/api/releases/${encodeURIComponent(releaseId)}/detail`,
	);
}

export async function apiTranslateReleaseDetail(
	releaseId: string,
): Promise<ReleaseDetailTranslated> {
	return apiPostJson<ReleaseDetailTranslated>("/api/translate/release/detail", {
		release_id: releaseId,
	});
}
