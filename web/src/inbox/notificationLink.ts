const GITHUB_NOTIFICATIONS_URL = "https://github.com/notifications";

export function resolveNotificationHref(notification: {
	html_url: string | null;
	repo_full_name: string | null;
}) {
	if (notification.html_url) {
		return notification.html_url;
	}
	if (notification.repo_full_name) {
		return `${GITHUB_NOTIFICATIONS_URL}?query=${encodeURIComponent(
			`repo:${notification.repo_full_name}`,
		)}`;
	}
	return GITHUB_NOTIFICATIONS_URL;
}
