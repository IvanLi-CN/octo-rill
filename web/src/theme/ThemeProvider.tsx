import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useState,
	type ReactNode,
} from "react";

import {
	applyThemeToDocument,
	getSystemResolvedTheme,
	persistThemePreference,
	readStoredThemePreference,
	resolveTheme,
	type ResolvedTheme,
	type ThemePreference,
	THEME_MEDIA_QUERY,
} from "@/theme/theme";

type ThemeContextValue = {
	themePreference: ThemePreference;
	resolvedTheme: ResolvedTheme;
	setThemePreference: (nextPreference: ThemePreference) => void;
};

type ThemeProviderProps = {
	children: ReactNode;
	defaultPreference?: ThemePreference;
	persist?: boolean;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function getInitialThemePreference(
	defaultPreference: ThemePreference,
	persist: boolean,
): ThemePreference {
	if (!persist) return defaultPreference;
	return readStoredThemePreference();
}

export function ThemeProvider({
	children,
	defaultPreference = "system",
	persist = true,
}: ThemeProviderProps) {
	const [themePreference, setThemePreferenceState] = useState<ThemePreference>(
		() => getInitialThemePreference(defaultPreference, persist),
	);
	const [resolvedTheme, setResolvedThemeState] = useState<ResolvedTheme>(() =>
		resolveTheme(
			getInitialThemePreference(defaultPreference, persist),
			getSystemResolvedTheme(),
		),
	);

	const setThemePreference = useCallback(
		(nextPreference: ThemePreference) => {
			const nextResolvedTheme = resolveTheme(
				nextPreference,
				getSystemResolvedTheme(),
			);
			setThemePreferenceState(nextPreference);
			setResolvedThemeState(nextResolvedTheme);

			if (typeof document !== "undefined") {
				applyThemeToDocument(document, nextPreference, nextResolvedTheme);
			}

			if (persist) {
				persistThemePreference(nextPreference);
			}
		},
		[persist],
	);

	useEffect(() => {
		if (typeof document !== "undefined") {
			applyThemeToDocument(document, themePreference, resolvedTheme);
		}

		if (persist) {
			persistThemePreference(themePreference);
		}
	}, [persist, resolvedTheme, themePreference]);

	useEffect(() => {
		if (
			typeof window === "undefined" ||
			typeof window.matchMedia !== "function"
		) {
			return;
		}

		const mediaQueryList = window.matchMedia(THEME_MEDIA_QUERY);

		const handleChange = () => {
			const nextResolvedTheme = resolveTheme(
				themePreference,
				mediaQueryList.matches ? "dark" : "light",
			);
			setResolvedThemeState(nextResolvedTheme);

			if (typeof document !== "undefined") {
				applyThemeToDocument(document, themePreference, nextResolvedTheme);
			}
		};

		handleChange();
		if (typeof mediaQueryList.addEventListener === "function") {
			mediaQueryList.addEventListener("change", handleChange);
		} else {
			mediaQueryList.addListener(handleChange);
		}

		return () => {
			if (typeof mediaQueryList.removeEventListener === "function") {
				mediaQueryList.removeEventListener("change", handleChange);
			} else {
				mediaQueryList.removeListener(handleChange);
			}
		};
	}, [themePreference]);

	const value = useMemo<ThemeContextValue>(
		() => ({
			themePreference,
			resolvedTheme,
			setThemePreference,
		}),
		[resolvedTheme, setThemePreference, themePreference],
	);

	return (
		<ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
	);
}

export function useTheme() {
	const value = useContext(ThemeContext);
	if (!value) {
		throw new Error("useTheme must be used within ThemeProvider");
	}
	return value;
}
