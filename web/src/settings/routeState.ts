export const SETTINGS_SECTIONS = [
	"linuxdo",
	"passkeys",
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
		passkey?: string | null;
	},
) {
	return {
		section: section === "linuxdo" ? undefined : section,
		linuxdo: options?.linuxdo ?? undefined,
		github: options?.github ?? undefined,
		passkey: options?.passkey ?? undefined,
	};
}

export function buildSettingsHref(
	section: SettingsSection,
	options?: {
		linuxdo?: string | null;
		github?: string | null;
		passkey?: string | null;
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
	if (search.passkey) {
		params.set("passkey", search.passkey);
	}
	const query = params.toString();
	return query ? `/settings?${query}` : "/settings";
}
