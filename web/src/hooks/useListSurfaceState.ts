import { useEffect, useMemo, useRef, useState } from "react";

export type ListSurfaceState =
	| "initial-loading"
	| "ready"
	| "refreshing"
	| "empty"
	| "blocking-error";

type UseListSurfaceStateOptions = {
	loading: boolean;
	hasData: boolean;
	hasError: boolean;
	delayMs?: number;
};

export function useListSurfaceState(options: UseListSurfaceStateOptions) {
	const { loading, hasData, hasError, delayMs = 400 } = options;
	const [showRefreshing, setShowRefreshing] = useState(false);
	const timerRef = useRef<number | null>(null);
	const hasResolvedOnceRef = useRef(false);
	const hasRequestedOnceRef = useRef(loading);

	useEffect(() => {
		if (loading) {
			hasRequestedOnceRef.current = true;
			return;
		}
		if (hasRequestedOnceRef.current || hasData || hasError) {
			hasResolvedOnceRef.current = true;
		}
	}, [hasData, hasError, loading]);

	useEffect(() => {
		if (!loading || !hasResolvedOnceRef.current) {
			setShowRefreshing(false);
			if (timerRef.current !== null) {
				window.clearTimeout(timerRef.current);
				timerRef.current = null;
			}
			return;
		}

		timerRef.current = window.setTimeout(
			() => {
				setShowRefreshing(true);
				timerRef.current = null;
			},
			Math.max(0, delayMs),
		);

		return () => {
			if (timerRef.current !== null) {
				window.clearTimeout(timerRef.current);
				timerRef.current = null;
			}
		};
	}, [delayMs, hasData, loading]);

	const state = useMemo<ListSurfaceState>(() => {
		if (loading && !hasData && !hasResolvedOnceRef.current) {
			return "initial-loading";
		}
		if (hasError && !hasData) return "blocking-error";
		if (!hasData) return "empty";
		if (showRefreshing) return "refreshing";
		return "ready";
	}, [hasData, hasError, loading, showRefreshing]);

	return {
		state,
		showRefreshing,
		dataState: state,
	};
}
