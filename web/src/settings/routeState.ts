export const SETTINGS_SECTIONS = [
	"linuxdo",
	"github-accounts",
	"my-releases",
	"github-pat",
	"daily-brief",
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export function normalizeSettingsSection(
	raw: string | null | undefined,
): SettingsSection {
	return SETTINGS_SECTIONS.find((section) => section === raw) ?? "linuxdo";
}

export function buildSettingsSearch(
	section: SettingsSection,
	options?: {
		linuxdo?: string | null;
		github?: string | null;
	},
) {
	return {
		section: section === "linuxdo" ? undefined : section,
		linuxdo: options?.linuxdo ?? undefined,
		github: options?.github ?? undefined,
	};
}

export function buildSettingsHref(
	section: SettingsSection,
	options?: {
		linuxdo?: string | null;
		github?: string | null;
	},
) {
	const params = new URLSearchParams();
	const search = buildSettingsSearch(section, options);
	if (search.section) {
		params.set("section", search.section);
	}
	if (search.linuxdo) {
		params.set("linuxdo", search.linuxdo);
	}
	if (search.github) {
		params.set("github", search.github);
	}
	const query = params.toString();
	return query ? `/settings?${query}` : "/settings";
}
