const ISO_TIMESTAMP_RE =
	/\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})\b/g;
const TIME_ZONE_FORMATTER_CACHE = new Map<string, Intl.DateTimeFormat>();

function pad2(value: number) {
	return value.toString().padStart(2, "0");
}

function parseIso(value: string | null | undefined) {
	if (!value) return null;
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function formatLocalDateTime(parsed: Date) {
	return `${parsed.getFullYear()}-${pad2(parsed.getMonth() + 1)}-${pad2(parsed.getDate())} ${pad2(parsed.getHours())}:${pad2(parsed.getMinutes())}:${pad2(parsed.getSeconds())}`;
}

export function formatIsoShortLocal(iso: string | null | undefined) {
	if (!iso) return "";
	const parsed = parseIso(iso);
	if (!parsed) return iso;
	return formatLocalDateTime(parsed);
}

export function formatIsoRangeLocal(
	start: string | null | undefined,
	end: string | null | undefined,
) {
	if (!start || !end) return null;
	return `${formatIsoShortLocal(start)} → ${formatIsoShortLocal(end)}`;
}

function getTimeZoneFormatter(timeZone: string) {
	const cached = TIME_ZONE_FORMATTER_CACHE.get(timeZone);
	if (cached) return cached;
	const formatter = new Intl.DateTimeFormat("sv-SE", {
		timeZone,
		year: "numeric",
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		hour12: false,
	});
	TIME_ZONE_FORMATTER_CACHE.set(timeZone, formatter);
	return formatter;
}

export function formatIsoShortInTimeZone(
	iso: string | null | undefined,
	timeZone: string | null | undefined,
) {
	if (!iso) return "";
	const parsed = parseIso(iso);
	if (!parsed) return iso;
	if (!timeZone) return formatLocalDateTime(parsed);
	try {
		return getTimeZoneFormatter(timeZone).format(parsed).replace(",", "");
	} catch {
		return formatLocalDateTime(parsed);
	}
}

export function formatIsoRangeInTimeZone(
	start: string | null | undefined,
	end: string | null | undefined,
	timeZone: string | null | undefined,
) {
	if (!start || !end) return null;
	return `${formatIsoShortInTimeZone(start, timeZone)} → ${formatIsoShortInTimeZone(end, timeZone)}`;
}

export function replaceIsoTimestampsWithLocal(text: string) {
	return text.replace(ISO_TIMESTAMP_RE, (match) => formatIsoShortLocal(match));
}
