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
