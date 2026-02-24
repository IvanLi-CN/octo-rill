export function normalizeReleaseId(
	raw: string | null | undefined,
): string | null {
	if (!raw) return null;
	const trimmed = raw.trim();
	if (!/^\d+$/.test(trimmed)) return null;
	try {
		return BigInt(trimmed).toString();
	} catch {
		return null;
	}
}
