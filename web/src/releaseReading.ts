export type ReleaseReading = {
	version: string;
	outcome: "changes" | "insufficient";
	title: string | null;
	changes: Array<{
		text: string;
		versions?: string[];
	}>;
	machine_materials?: Array<{
		label: string;
		value: string;
	}>;
};

export function releaseReadingChangeVersions(
	readingVersion: string,
	versions: string[] | undefined,
) {
	const unique = Array.from(
		new Set(
			(versions ?? [])
				.map((version) => version.trim())
				.filter((version) => version.length > 0),
		),
	);
	if (unique.length === 0) return null;
	if (unique.length === 1 && unique[0] === readingVersion.trim()) return null;
	return unique.join("、");
}
