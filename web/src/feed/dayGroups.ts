import { isReleaseFeedItem, type FeedItem } from "@/feed/types";

const DEFAULT_DAILY_BOUNDARY = "08:00";
const timeZoneFormatterCache = new Map<string, Intl.DateTimeFormat>();

export type DailyBoundaryLocal = {
	hour: number;
	minute: number;
	label: string;
};

export type FeedDayGroup = {
	kind: "raw" | "brief";
	id: string;
	displayDate: string;
	briefDate: string;
	briefId: string | null;
	items: FeedItem[];
	itemCount: number;
	releaseCount: number;
	activityCount: number;
	isCurrent: boolean;
};

export type BriefSnapshotCandidate = {
	id: string;
	date: string;
	window_start?: string | null;
	window_end?: string | null;
	release_ids?: string[];
};

function pad2(value: number) {
	return value.toString().padStart(2, "0");
}

function formatLocalDateKey(value: Date) {
	return `${value.getFullYear()}-${pad2(value.getMonth() + 1)}-${pad2(value.getDate())}`;
}

function formatUtcDateKey(value: Date) {
	return `${value.getUTCFullYear()}-${pad2(value.getUTCMonth() + 1)}-${pad2(value.getUTCDate())}`;
}

function shiftDateKey(dateKey: string, days: number) {
	const [year, month, day] = dateKey.split("-").map(Number);
	if (
		!Number.isInteger(year) ||
		!Number.isInteger(month) ||
		!Number.isInteger(day)
	) {
		return dateKey;
	}
	const shifted = new Date(Date.UTC(year, month - 1, day));
	shifted.setUTCDate(shifted.getUTCDate() + days);
	return formatUtcDateKey(shifted);
}

function getTimeZoneFormatter(timeZone: string) {
	const cached = timeZoneFormatterCache.get(timeZone);
	if (cached) return cached;

	const formatter = new Intl.DateTimeFormat("en-CA", {
		timeZone,
		year: "numeric",
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		hourCycle: "h23",
	});
	timeZoneFormatterCache.set(timeZone, formatter);
	return formatter;
}

function getTimeZoneParts(value: Date, timeZone: string) {
	try {
		const parts = getTimeZoneFormatter(timeZone).formatToParts(value);
		const lookup = new Map(parts.map((part) => [part.type, part.value]));
		const year = Number(lookup.get("year"));
		const month = Number(lookup.get("month"));
		const day = Number(lookup.get("day"));
		const hour = Number(lookup.get("hour"));
		const minute = Number(lookup.get("minute"));
		const second = Number(lookup.get("second"));
		if (
			![year, month, day, hour, minute, second].every((part) =>
				Number.isInteger(part),
			)
		) {
			return null;
		}
		return { year, month, day, hour, minute, second };
	} catch {
		return null;
	}
}

function getFixedOffsetParts(value: Date, utcOffsetMinutes: number) {
	if (!Number.isFinite(utcOffsetMinutes)) {
		return null;
	}

	const shifted = new Date(value.getTime() + utcOffsetMinutes * 60 * 1000);
	return {
		year: shifted.getUTCFullYear(),
		month: shifted.getUTCMonth() + 1,
		day: shifted.getUTCDate(),
		hour: shifted.getUTCHours(),
		minute: shifted.getUTCMinutes(),
		second: shifted.getUTCSeconds(),
	};
}

export function parseDailyBoundaryLocal(
	rawValue: string | null | undefined,
): DailyBoundaryLocal {
	const match = rawValue?.match(/^(\d{2}):(\d{2})$/);
	if (!match) {
		return { hour: 8, minute: 0, label: DEFAULT_DAILY_BOUNDARY };
	}

	const hour = Number(match[1]);
	const minute = Number(match[2]);
	if (
		!Number.isInteger(hour) ||
		!Number.isInteger(minute) ||
		hour < 0 ||
		hour > 23 ||
		minute < 0 ||
		minute > 59
	) {
		return { hour: 8, minute: 0, label: DEFAULT_DAILY_BOUNDARY };
	}

	return {
		hour,
		minute,
		label: `${pad2(hour)}:${pad2(minute)}`,
	};
}

function resolveWindowStart(value: Date, boundary: DailyBoundaryLocal) {
	const start = new Date(value);
	start.setHours(boundary.hour, boundary.minute, 0, 0);
	if (value < start) {
		start.setDate(start.getDate() - 1);
	}
	return start;
}

function resolveWindowStartDateKey(
	value: Date,
	boundary: DailyBoundaryLocal,
	timeZone: string | null | undefined,
	utcOffsetMinutes: number | null | undefined,
) {
	const zoned =
		(timeZone ? getTimeZoneParts(value, timeZone) : null) ??
		(utcOffsetMinutes != null
			? getFixedOffsetParts(value, utcOffsetMinutes)
			: null);
	if (zoned) {
		const dateKey = `${zoned.year}-${pad2(zoned.month)}-${pad2(zoned.day)}`;
		const isBeforeBoundary =
			zoned.hour < boundary.hour ||
			(zoned.hour === boundary.hour && zoned.minute < boundary.minute);
		return isBeforeBoundary ? shiftDateKey(dateKey, -1) : dateKey;
	}

	return formatLocalDateKey(resolveWindowStart(value, boundary));
}

export function groupFeedItemsByDay(
	items: FeedItem[],
	dailyBoundaryLocal: string | null | undefined,
	dailyBoundaryTimeZone: string | null | undefined,
	dailyBoundaryUtcOffsetMinutes: number | null | undefined,
	briefs: BriefSnapshotCandidate[] = [],
	now = new Date(),
) {
	const boundary = parseDailyBoundaryLocal(dailyBoundaryLocal);
	const currentWindowStartKey = resolveWindowStartDateKey(
		now,
		boundary,
		dailyBoundaryTimeZone,
		dailyBoundaryUtcOffsetMinutes,
	);
	const currentGroupId = `${currentWindowStartKey}@${boundary.label}`;
	const historicalBriefOrder = new Map<string, number>();
	const historicalBriefById = new Map<string, BriefSnapshotCandidate>();
	const releaseToHistoricalBriefIds = new Map<string, string[]>();
	const historicalBriefWindows: Array<{
		brief: BriefSnapshotCandidate;
		windowStartMs: number;
		windowEndMs: number;
	}> = [];

	for (const [index, brief] of briefs.entries()) {
		if (!brief.window_start || !brief.window_end) continue;
		const windowStart = new Date(brief.window_start);
		const windowEnd = new Date(brief.window_end);
		if (
			Number.isNaN(windowStart.getTime()) ||
			Number.isNaN(windowEnd.getTime()) ||
			windowEnd >= now
		) {
			continue;
		}
		historicalBriefOrder.set(brief.id, index);
		historicalBriefById.set(brief.id, brief);
		for (const releaseId of brief.release_ids ?? []) {
			const existing = releaseToHistoricalBriefIds.get(releaseId);
			if (existing) {
				if (!existing.includes(brief.id)) {
					existing.push(brief.id);
				}
			} else {
				releaseToHistoricalBriefIds.set(releaseId, [brief.id]);
			}
		}
		historicalBriefWindows.push({
			brief,
			windowStartMs: windowStart.getTime(),
			windowEndMs: windowEnd.getTime(),
		});
	}
	historicalBriefWindows.sort(
		(left, right) =>
			right.windowEndMs - left.windowEndMs ||
			right.windowStartMs - left.windowStartMs ||
			right.brief.id.localeCompare(left.brief.id),
	);
	const pickCanonicalHistoricalBrief = (briefIds: string[]) => {
		let bestBrief: BriefSnapshotCandidate | null = null;
		let bestRank = Number.POSITIVE_INFINITY;
		for (const briefId of briefIds) {
			const brief = historicalBriefById.get(briefId);
			if (!brief) continue;
			const rank =
				historicalBriefOrder.get(briefId) ?? Number.POSITIVE_INFINITY;
			if (rank < bestRank) {
				bestBrief = brief;
				bestRank = rank;
			}
		}
		return bestBrief;
	};
	const groups = new Map<string, FeedDayGroup>();
	const appendHistoricalBriefItem = (
		brief: BriefSnapshotCandidate,
		item: FeedItem,
		countType: "release" | "activity",
	) => {
		const groupId = `brief:${brief.id}`;
		const existing = groups.get(groupId);
		if (existing) {
			existing.items.push(item);
			existing.itemCount += 1;
			if (countType === "release") {
				existing.releaseCount += 1;
			} else {
				existing.activityCount += 1;
			}
			return;
		}
		groups.set(groupId, {
			kind: "brief",
			id: groupId,
			displayDate: brief.date,
			briefDate: brief.date,
			briefId: brief.id,
			items: [item],
			itemCount: 1,
			releaseCount: countType === "release" ? 1 : 0,
			activityCount: countType === "activity" ? 1 : 0,
			isCurrent: false,
		});
	};

	for (const item of items) {
		if (isReleaseFeedItem(item)) {
			const canonicalBrief = pickCanonicalHistoricalBrief(
				releaseToHistoricalBriefIds.get(item.id) ?? [],
			);
			if (canonicalBrief) {
				appendHistoricalBriefItem(canonicalBrief, item, "release");
				continue;
			}
		}

		const publishedAt = new Date(item.ts);
		if (Number.isNaN(publishedAt.getTime())) {
			const unknownKey = "unknown";
			const existing = groups.get(unknownKey);
			if (existing) {
				existing.items.push(item);
				existing.itemCount += 1;
				if (isReleaseFeedItem(item)) {
					existing.releaseCount += 1;
				} else {
					existing.activityCount += 1;
				}
				continue;
			}
			groups.set(unknownKey, {
				kind: "raw",
				id: unknownKey,
				displayDate: "未知日期",
				briefDate: "",
				briefId: null,
				items: [item],
				itemCount: 1,
				releaseCount: isReleaseFeedItem(item) ? 1 : 0,
				activityCount: isReleaseFeedItem(item) ? 0 : 1,
				isCurrent: false,
			});
			continue;
		}

		const windowStartKey = resolveWindowStartDateKey(
			publishedAt,
			boundary,
			dailyBoundaryTimeZone,
			dailyBoundaryUtcOffsetMinutes,
		);
		const briefDate = shiftDateKey(windowStartKey, 1);
		if (!isReleaseFeedItem(item)) {
			const itemTime = publishedAt.getTime();
			const canonicalBrief = historicalBriefWindows.find(
				(candidate) =>
					itemTime >= candidate.windowStartMs &&
					itemTime < candidate.windowEndMs,
			)?.brief;
			if (canonicalBrief) {
				appendHistoricalBriefItem(canonicalBrief, item, "activity");
				continue;
			}
		}
		const groupId = `${windowStartKey}@${boundary.label}`;
		const existing = groups.get(groupId);
		if (existing) {
			existing.items.push(item);
			existing.itemCount += 1;
			if (isReleaseFeedItem(item)) {
				existing.releaseCount += 1;
			} else {
				existing.activityCount += 1;
			}
			continue;
		}

		groups.set(groupId, {
			kind: "raw",
			id: groupId,
			displayDate: windowStartKey,
			briefDate,
			briefId: null,
			items: [item],
			itemCount: 1,
			releaseCount: isReleaseFeedItem(item) ? 1 : 0,
			activityCount: isReleaseFeedItem(item) ? 0 : 1,
			isCurrent: groupId === currentGroupId,
		});
	}

	return Array.from(groups.values());
}
