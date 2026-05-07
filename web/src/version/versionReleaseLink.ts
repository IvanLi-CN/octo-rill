const PROJECT_RELEASE_OWNER = "IvanLi-CN";
const PROJECT_RELEASE_REPO = "octo-rill";
const SEMVER_RELEASE_TAG_PATTERN =
	/^(?:v)?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

export function buildVersionReleaseHref(version: string): string | null {
	const tag = buildVersionReleaseTag(version);
	if (!tag) {
		return null;
	}

	return `/public/${PROJECT_RELEASE_OWNER}/${PROJECT_RELEASE_REPO}/releases/tag/${encodeURIComponent(tag)}`;
}

export function buildVersionReleaseTag(version: string): string | null {
	const tag = version.trim();
	if (!tag || tag === "unknown") {
		return null;
	}
	if (SEMVER_RELEASE_TAG_PATTERN.test(tag) && !tag.startsWith("v")) {
		return `v${tag}`;
	}
	return tag;
}
