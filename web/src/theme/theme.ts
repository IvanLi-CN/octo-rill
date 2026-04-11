export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "octo-rill.theme-preference";
export const THEME_MEDIA_QUERY = "(prefers-color-scheme: dark)";

export function isThemePreference(value: unknown): value is ThemePreference {
	return value === "system" || value === "light" || value === "dark";
}

export function normalizeThemePreference(
	value: string | null | undefined,
): ThemePreference {
	return isThemePreference(value) ? value : "system";
}

export function getSystemResolvedTheme(
	targetWindow: Window | undefined = typeof window !== "undefined"
		? window
		: undefined,
): ResolvedTheme {
	if (!targetWindow || typeof targetWindow.matchMedia !== "function") {
		return "light";
	}

	return targetWindow.matchMedia(THEME_MEDIA_QUERY).matches ? "dark" : "light";
}

export function resolveTheme(
	preference: ThemePreference,
	systemTheme: ResolvedTheme,
): ResolvedTheme {
	return preference === "system" ? systemTheme : preference;
}

export function readStoredThemePreference(storage?: Storage): ThemePreference {
	try {
		const targetStorage =
			storage ??
			(typeof window !== "undefined" ? window.localStorage : undefined);
		if (!targetStorage) return "system";

		return normalizeThemePreference(targetStorage.getItem(THEME_STORAGE_KEY));
	} catch {
		return "system";
	}
}

export function persistThemePreference(
	preference: ThemePreference,
	storage?: Storage,
): void {
	try {
		const targetStorage =
			storage ??
			(typeof window !== "undefined" ? window.localStorage : undefined);
		if (!targetStorage) return;

		targetStorage.setItem(THEME_STORAGE_KEY, preference);
	} catch {
		// ignore storage failures
	}
}

export function applyThemeToDocument(
	targetDocument: Document,
	preference: ThemePreference,
	resolvedTheme: ResolvedTheme,
): void {
	const root = targetDocument.documentElement;
	root.classList.toggle("dark", resolvedTheme === "dark");
	root.dataset.themePreference = preference;
	root.dataset.theme = resolvedTheme;
	root.style.colorScheme = resolvedTheme;
}
