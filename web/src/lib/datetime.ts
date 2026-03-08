function pad2(value: number) {
	return value.toString().padStart(2, "0");
}

export function formatIsoShortLocal(iso: string | null | undefined) {
	if (!iso) return "";
	const parsed = new Date(iso);
	if (Number.isNaN(parsed.getTime())) return iso;
	return `${parsed.getFullYear()}-${pad2(parsed.getMonth() + 1)}-${pad2(parsed.getDate())} ${pad2(parsed.getHours())}:${pad2(parsed.getMinutes())}:${pad2(parsed.getSeconds())}`;
}
