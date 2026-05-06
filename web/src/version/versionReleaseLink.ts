const PROJECT_RELEASE_OWNER = "IvanLi-CN";
const PROJECT_RELEASE_REPO = "octo-rill";

export function buildVersionReleaseHref(version: string): string | null {
	const tag = version.trim();
	if (!tag || tag === "unknown") {
		return null;
	}

	return `/${PROJECT_RELEASE_OWNER}/${PROJECT_RELEASE_REPO}/releases/tag/${encodeURIComponent(tag)}`;
}
