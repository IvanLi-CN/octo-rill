import { useEffect, useState } from "react";

function readMatch(query: string, fallback: boolean) {
	if (
		typeof window === "undefined" ||
		typeof window.matchMedia !== "function"
	) {
		return fallback;
	}
	return window.matchMedia(query).matches;
}

export function useMediaQuery(query: string, fallback = false) {
	const [matches, setMatches] = useState<boolean>(() =>
		readMatch(query, fallback),
	);

	useEffect(() => {
		if (
			typeof window === "undefined" ||
			typeof window.matchMedia !== "function"
		) {
			setMatches(fallback);
			return;
		}

		const mediaQuery = window.matchMedia(query);
		const update = () => setMatches(mediaQuery.matches);
		update();
		mediaQuery.addEventListener("change", update);
		return () => {
			mediaQuery.removeEventListener("change", update);
		};
	}, [fallback, query]);

	return matches;
}
