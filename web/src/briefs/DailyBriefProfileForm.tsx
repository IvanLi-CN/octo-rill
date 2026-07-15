import { useId, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
const TIME_ZONE_SAMPLE_YEAR = 2026;
const TIME_ZONE_SAMPLE_DAYS = 400;
const browserTimeZoneSupportCache = new Map<string, boolean>();

function readSupportedTimeZones() {
	const supportedValuesOf = (
		Intl as typeof Intl & {
			supportedValuesOf?: (key: string) => string[];
		}
	).supportedValuesOf;
	if (typeof supportedValuesOf !== "function") {
		return [] as string[];
	}
	try {
		return supportedValuesOf("timeZone");
	} catch {
		return [] as string[];
	}
}

export function readBrowserTimeZone() {
	return Intl.DateTimeFormat().resolvedOptions().timeZone || "Asia/Shanghai";
}

function parseShortOffsetMinutes(raw: string) {
	const normalized = raw.trim().toUpperCase();
	if (normalized === "GMT" || normalized === "UTC") {
		return 0;
	}
	const match = normalized.match(/^GMT([+-])(\d{1,2})(?::?(\d{2}))?$/);
	if (!match) {
		return null;
	}
	const [, sign, hoursRaw, minutesRaw] = match;
	const hours = Number(hoursRaw);
	const minutes = Number(minutesRaw ?? "0");
	if (!Number.isFinite(hours) || !Number.isFinite(minutes)) {
		return null;
	}
	const totalMinutes = hours * 60 + minutes;
	return sign === "-" ? -totalMinutes : totalMinutes;
}

function readOffsetMinutesAt(timeZone: string, date: Date) {
	try {
		const parts = new Intl.DateTimeFormat("en-US", {
			timeZone,
			timeZoneName: "shortOffset",
			hour: "2-digit",
			minute: "2-digit",
			hour12: false,
		}).formatToParts(date);
		const offset = parts.find((part) => part.type === "timeZoneName")?.value;
		return offset ? parseShortOffsetMinutes(offset) : null;
	} catch {
		return null;
	}
}

export function isHourAlignedTimeZone(timeZone: string) {
	const cached = browserTimeZoneSupportCache.get(timeZone);
	if (cached !== undefined) {
		return cached;
	}

	for (let dayOffset = 0; dayOffset <= TIME_ZONE_SAMPLE_DAYS; dayOffset += 1) {
		const probe = new Date(
			Date.UTC(TIME_ZONE_SAMPLE_YEAR, 0, 1 + dayOffset, 12, 0, 0),
		);
		const offsetMinutes = readOffsetMinutesAt(timeZone, probe);
		if (offsetMinutes === null || offsetMinutes % 60 !== 0) {
			browserTimeZoneSupportCache.set(timeZone, false);
			return false;
		}
	}

	browserTimeZoneSupportCache.set(timeZone, true);
	return true;
}

export function readHourAlignedBrowserTimeZone() {
	const browserTimeZone = readBrowserTimeZone();
	return isHourAlignedTimeZone(browserTimeZone) ? browserTimeZone : null;
}

const SUPPORTED_TIME_ZONES = readSupportedTimeZones();

export function DailyBriefProfileForm(props: {
	timeZone: string;
	disabled?: boolean;
	error?: string | null;
	helperText?: ReactNode;
	compact?: boolean;
	onTimeZoneChange: (value: string) => void;
	onUseBrowserTimeZone?: (timeZone: string) => void;
}) {
	const {
		timeZone,
		disabled = false,
		error,
		helperText,
		compact = false,
		onTimeZoneChange,
		onUseBrowserTimeZone,
	} = props;
	const timeZoneListId = useId();
	const browserTimeZone = readBrowserTimeZone();
	const supportedBrowserTimeZone = readHourAlignedBrowserTimeZone();

	return (
		<div className="space-y-4">
			<div className="space-y-2">
				<div className="space-y-2">
					<div className="flex items-center justify-between gap-2">
						<Label htmlFor={`${timeZoneListId}-zone`}>IANA 时区</Label>
						{onUseBrowserTimeZone ? (
							<Button
								type="button"
								variant="outline"
								size="sm"
								disabled={disabled || !supportedBrowserTimeZone}
								onClick={() => {
									if (supportedBrowserTimeZone) {
										onUseBrowserTimeZone(supportedBrowserTimeZone);
									}
								}}
							>
								使用浏览器时区
							</Button>
						) : null}
					</div>
					<Input
						id={`${timeZoneListId}-zone`}
						list={SUPPORTED_TIME_ZONES.length > 0 ? timeZoneListId : undefined}
						value={timeZone}
						disabled={disabled}
						onChange={(event) => onTimeZoneChange(event.target.value)}
						placeholder="例如 Asia/Shanghai"
						autoCapitalize="none"
						autoCorrect="off"
						spellCheck={false}
					/>
					{SUPPORTED_TIME_ZONES.length > 0 ? (
						<datalist id={timeZoneListId}>
							{SUPPORTED_TIME_ZONES.map((zone) => (
								<option key={zone} value={zone} />
							))}
						</datalist>
					) : null}
					<p className="text-muted-foreground text-xs">
						浏览器当前识别为 <code>{browserTimeZone}</code>
						{supportedBrowserTimeZone
							? "；日报会按这个时区解释“昨天”自然日。"
							: "；该时区当前不满足“全年整点 UTC 偏移”约束，请手动选择受支持的 IANA 时区。"}
					</p>
				</div>
			</div>

			{helperText ? (
				<div
					className={cn(
						"text-muted-foreground text-xs leading-5",
						compact
							? "rounded-lg border border-border/60 bg-muted/20 px-3 py-2"
							: "rounded-lg border p-3",
					)}
				>
					{helperText}
				</div>
			) : null}
			{error ? <p className="text-destructive text-sm">{error}</p> : null}
		</div>
	);
}
