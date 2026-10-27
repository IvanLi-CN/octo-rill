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
