export type ErrorPresentationLike = {
	error?: string | null;
	error_summary?: string | null;
	error_detail?: string | null;
	message?: string | null;
};

export function describeUnknownError(
	error: unknown,
	fallback = "操作失败，请稍后重试。",
) {
	if (error instanceof Error) {
		const message = error.message.trim();
		return message || fallback;
	}
	if (typeof error === "string") {
		const message = error.trim();
		return message || fallback;
	}
	return fallback;
}

export function resolveErrorSummary(
	value: ErrorPresentationLike | string | null | undefined,
	fallback = "操作失败，请稍后重试。",
) {
	if (typeof value === "string") {
		const message = value.trim();
		return message || fallback;
	}
	if (!value) {
		return fallback;
	}
	const summary =
		value.error_summary?.trim() || value.error?.trim() || value.message?.trim();
	return summary || fallback;
}

export function resolveErrorDetail(
	value: ErrorPresentationLike | null | undefined,
) {
	if (!value) {
		return null;
	}
	const detail = value.error_detail?.trim();
	if (detail && detail.length > 0) {
		return detail;
	}
	return null;
}
