const GITHUB_NOTIFICATIONS_URL = "https://github.com/notifications";
const GITHUB_WEB_BASE = "https://github.com";

function normalizeGithubUrl(value: string) {
	try {
		const parsed = new URL(value);
		if (parsed.origin !== "https://github.com") {
			return null;
		}
		return `${parsed.origin}${parsed.pathname.replace(/\/+$/, "")}`;
	} catch {
		return null;
	}
}

function notificationThreadHref(threadId: string) {
	const trimmed = threadId.trim();
	if (!trimmed) {
		return null;
	}
	return `${GITHUB_NOTIFICATIONS_URL}/threads/${trimmed}`;
}

function isFallbackNotificationHref(notification: {
	thread_id: string;
	html_url: string | null;
	repo_full_name: string | null;
}) {
	if (!notification.html_url || !notification.repo_full_name) {
		const normalized = notification.html_url
			? normalizeGithubUrl(notification.html_url)
			: null;
		const threadHref = notificationThreadHref(notification.thread_id);
		return (
			normalized === GITHUB_NOTIFICATIONS_URL &&
			(!threadHref || normalized !== threadHref)
		);
	}
	const normalized = normalizeGithubUrl(notification.html_url);
	const threadHref = notificationThreadHref(notification.thread_id);
	return (
		(normalized === GITHUB_NOTIFICATIONS_URL &&
			(!threadHref || normalized !== threadHref)) ||
		normalized === `${GITHUB_WEB_BASE}/${notification.repo_full_name}`
	);
}

export function resolveNotificationHref(notification: {
	thread_id: string;
	html_url: string | null;
	repo_full_name: string | null;
}) {
	if (notification.html_url && !isFallbackNotificationHref(notification)) {
		return notification.html_url;
	}
	const threadHref = notificationThreadHref(notification.thread_id);
	if (threadHref) {
		return threadHref;
	}
	if (notification.repo_full_name) {
		return `${GITHUB_NOTIFICATIONS_URL}?query=${encodeURIComponent(
			`repo:${notification.repo_full_name}`,
		)}`;
	}
	return GITHUB_NOTIFICATIONS_URL;
}
