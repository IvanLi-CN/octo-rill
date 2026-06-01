export type ErrorPresentationLike = {
	error?: string | null;
	error_summary?: string | null;
	error_detail?: string | null;
	message?: string | null;
};

export type NetworkErrorKind = "offline" | "network" | "unknown";

export type NetworkAwareErrorDescription = {
	kind: NetworkErrorKind;
	message: string;
	detail: string | null;
};

function browserIsOffline() {
	return typeof navigator !== "undefined" && navigator.onLine === false;
}

function looksLikeNetworkFailure(error: unknown) {
	if (!(error instanceof Error)) return false;
	if (error.name === "AbortError") return false;
	if (error instanceof TypeError) return true;
	const message = error.message.toLowerCase();
	return (
		message.includes("failed to fetch") ||
		message.includes("load failed") ||
		message.includes("networkerror") ||
		message.includes("network request failed")
	);
}

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

export function describeNetworkAwareError(
	error: unknown,
	fallback = "操作失败，请稍后重试。",
): NetworkAwareErrorDescription {
	const detail = error instanceof Error ? error.message : null;
	if (browserIsOffline()) {
		return {
			kind: "offline",
			message:
				"当前处于离线状态，登录和最新数据需要网络连接；已保留可用的应用壳。",
			detail,
		};
	}
	if (looksLikeNetworkFailure(error)) {
		return {
			kind: "network",
			message: "暂时无法连接 OctoRill 服务，请检查网络后重试。",
			detail,
		};
	}
	return {
		kind: "unknown",
		message: describeUnknownError(error, fallback),
		detail,
	};
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
