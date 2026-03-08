const ISO_TIMESTAMP_RE =
	/\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})\b/g;

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

export function replaceIsoTimestampsWithLocal(text: string) {
	return text.replace(ISO_TIMESTAMP_RE, (match) => formatIsoShortLocal(match));
}
