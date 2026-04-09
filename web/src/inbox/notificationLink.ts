const GITHUB_NOTIFICATIONS_URL = "https://github.com/notifications";

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

function isStaleRepoHomepageHref(notification: {
	html_url: string | null;
	repo_full_name: string | null;
}) {
	if (!notification.html_url || !notification.repo_full_name) {
		return false;
	}
	return (
		normalizeGithubUrl(notification.html_url) ===
		`${GITHUB_NOTIFICATIONS_URL.replace("/notifications", "")}/${notification.repo_full_name}`
	);
}

export function resolveNotificationHref(notification: {
	html_url: string | null;
	repo_full_name: string | null;
}) {
	if (notification.html_url && !isStaleRepoHomepageHref(notification)) {
		return notification.html_url;
	}
	if (notification.repo_full_name) {
		return `${GITHUB_NOTIFICATIONS_URL}?query=${encodeURIComponent(
			`repo:${notification.repo_full_name}`,
		)}`;
	}
	return GITHUB_NOTIFICATIONS_URL;
}
