import { useEffect, useRef, useState } from "react";

import { useMediaQuery } from "@/lib/useMediaQuery";

export type DashboardSyncLifecycle =
	| "idle"
	| "running"
	| "refreshing"
	| "succeeded"
	| "failed";

export type DashboardSyncProgressInput = {
	currentStep: number;
	totalSteps: number;
};

export type DashboardSyncProgressBounds = {
	confirmed: number;
	next: number;
};

export type DashboardSyncPrediction = {
	percentage: number;
	confirmedPercentage: number;
	ceilingPercentage: number;
	active: boolean;
	complete: boolean;
};

export const DASHBOARD_SYNC_PREDICTION_90_PERCENT_MS = 12_000;
export const DASHBOARD_SYNC_SUCCESS_FILL_MS = 650;

const STAGE_BOUNDARY_GAP = 0.006;
const REFRESH_COMPLETION_CAP = 0.985;
const STOP_EPSILON = 0.0001;

function clampUnit(value: number) {
	return Math.max(0, Math.min(1, value));
}

function finiteOr(value: number, fallback: number) {
	return Number.isFinite(value) ? value : fallback;
}

export function resolveDashboardSyncProgressBounds(
	progress: DashboardSyncProgressInput | null | undefined,
): DashboardSyncProgressBounds {
	const totalSteps = Math.max(
		1,
		Math.floor(finiteOr(progress?.totalSteps ?? 0, 4)),
	);
	const currentStep = clampUnit(
		finiteOr(progress?.currentStep ?? 0, 0) / totalSteps,
	);
	return {
		confirmed: currentStep,
		next: Math.min(1, (Math.floor(currentStep * totalSteps) + 1) / totalSteps),
	};
}

export function advanceDashboardSyncPrediction(
	current: number,
	target: number,
	elapsedMs: number,
	lifecycle: DashboardSyncLifecycle,
): number {
	const from = clampUnit(current);
	const to = clampUnit(target);
	if (to <= from) return from;
	const elapsed = Math.max(0, Math.min(elapsedMs, 250));
	if (lifecycle === "succeeded") {
		return Math.min(
			to,
			from + (to - from) * (elapsed / DASHBOARD_SYNC_SUCCESS_FILL_MS),
		);
	}
	const rate = -Math.log(0.1) / DASHBOARD_SYNC_PREDICTION_90_PERCENT_MS;
	const blend = 1 - Math.exp(-elapsed * rate);
	return Math.min(to, from + (to - from) * blend);
}

function isActiveLifecycle(lifecycle: DashboardSyncLifecycle) {
	return lifecycle === "running" || lifecycle === "refreshing";
}

function resolveTarget(
	bounds: DashboardSyncProgressBounds,
	lifecycle: DashboardSyncLifecycle,
	current: number,
) {
	switch (lifecycle) {
		case "running":
			return Math.min(
				REFRESH_COMPLETION_CAP,
				Math.max(bounds.confirmed, bounds.next - STAGE_BOUNDARY_GAP),
			);
		case "refreshing":
			return REFRESH_COMPLETION_CAP;
		case "succeeded":
			return 1;
		case "failed":
			return current;
		default:
			return 0;
	}
}

export function useDashboardSyncPrediction(options: {
	progress: DashboardSyncProgressInput | null | undefined;
	lifecycle?: DashboardSyncLifecycle;
}): DashboardSyncPrediction {
	const { progress, lifecycle = "idle" } = options;
	const reducedMotion = useMediaQuery("(prefers-reduced-motion: reduce)");
	const bounds = resolveDashboardSyncProgressBounds(progress);
	const [percentage, setPercentage] = useState(0);
	const percentageRef = useRef(0);
	const confirmedRef = useRef(0);
	const targetRef = useRef(0);
	const previousLifecycleRef = useRef<DashboardSyncLifecycle>(lifecycle);

	useEffect(() => {
		const previousLifecycle = previousLifecycleRef.current;
		const started =
			lifecycle === "running" && !isActiveLifecycle(previousLifecycle);
		if (lifecycle === "idle") {
			percentageRef.current = 0;
			confirmedRef.current = 0;
			targetRef.current = 0;
			setPercentage(0);
			previousLifecycleRef.current = lifecycle;
			return;
		}

		if (started) {
			percentageRef.current = bounds.confirmed;
			confirmedRef.current = bounds.confirmed;
		} else {
			confirmedRef.current = Math.max(confirmedRef.current, bounds.confirmed);
		}

		targetRef.current = resolveTarget(
			{
				confirmed: confirmedRef.current,
				next: Math.max(bounds.next, confirmedRef.current),
			},
			lifecycle,
			percentageRef.current,
		);
		if (reducedMotion) {
			const reducedValue =
				lifecycle === "succeeded"
					? 1
					: Math.min(
							targetRef.current,
							Math.max(percentageRef.current, confirmedRef.current),
						);
			percentageRef.current = Math.max(percentageRef.current, reducedValue);
			setPercentage(percentageRef.current);
		}
		previousLifecycleRef.current = lifecycle;
	}, [bounds.confirmed, bounds.next, lifecycle, reducedMotion]);

	useEffect(() => {
		if (
			reducedMotion ||
			(!isActiveLifecycle(lifecycle) && lifecycle !== "succeeded")
		) {
			return;
		}

		let frame = 0;
		let previousTime = performance.now();
		const tick = (time: number) => {
			const elapsed = time - previousTime;
			previousTime = time;
			const current = percentageRef.current;
			const target = targetRef.current;
			const next = Math.max(
				current,
				advanceDashboardSyncPrediction(current, target, elapsed, lifecycle),
			);
			if (next !== current) {
				percentageRef.current = next;
				setPercentage(next);
			}
			if (target - next <= STOP_EPSILON) {
				if (lifecycle === "succeeded" && next < 1) {
					percentageRef.current = 1;
					setPercentage(1);
				}
				return;
			}
			frame = requestAnimationFrame(tick);
		};

		frame = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(frame);
	}, [lifecycle, reducedMotion]);

	const ceiling =
		lifecycle === "running"
			? targetRef.current
			: lifecycle === "refreshing"
				? REFRESH_COMPLETION_CAP
				: lifecycle === "succeeded"
					? 1
					: percentageRef.current;

	return {
		percentage,
		confirmedPercentage: confirmedRef.current,
		ceilingPercentage: Math.max(percentage, ceiling),
		active: isActiveLifecycle(lifecycle),
		complete: lifecycle === "succeeded" && percentage >= 1 - STOP_EPSILON,
	};
}
