const TRANSLATION_RETRYABLE_ERROR_FRAGMENTS = [
	"runtime_lease_expired",
	"repo scope required; re-login via github oauth",
	"database is locked",
	"busy",
	"ai returned 429",
	"too many requests",
	"rate limit",
	"rpm exhausted",
	"token plan limit exhausted",
	"timed out",
	"timeout",
	"temporarily unavailable",
	"connection reset",
	"connection refused",
	"error decoding response body",
	"error sending request for url",
	"ai response missing content",
	"transport request failed",
	"connection closed abruptly",
	"could not resolve host",
	"tls connect error",
	"connect tunnel failed, response 503",
	"chat upstream returned 500",
	"chat upstream returned 403",
];

export function translationErrorIsRetryable(error?: string | null) {
	if (!error) return false;
	const normalized = error.trim().toLowerCase();
	return (
		TRANSLATION_RETRYABLE_ERROR_FRAGMENTS.some((fragment) =>
			normalized.includes(fragment),
		) ||
		(normalized.includes("403 forbidden") &&
			(normalized.includes("chat") || normalized.includes("upstream")))
	);
}
