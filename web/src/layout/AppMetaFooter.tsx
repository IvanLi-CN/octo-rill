import { Github } from "lucide-react";
import { useEffect, useState } from "react";

type HealthResponse = {
	ok: boolean;
	version: string;
};

const REPOSITORY_URL = "https://github.com/IvanLi-CN/octo-rill";
const VERSION_LOADING = "loading...";
const VERSION_UNKNOWN = "unknown";

function normalizeVersion(raw: string): string {
	const trimmed = raw.trim();
	if (!trimmed) return VERSION_UNKNOWN;
	if (
		/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(trimmed)
	) {
		return `v${trimmed}`;
	}
	return trimmed;
}

export function AppMetaFooter() {
	const [version, setVersion] = useState(VERSION_LOADING);

	useEffect(() => {
		const abortController = new AbortController();
		let active = true;

		void (async () => {
			try {
				const response = await fetch("/api/health", {
					credentials: "include",
					signal: abortController.signal,
				});
				if (!response.ok) {
					throw new Error(`health request failed: ${response.status}`);
				}

				const payload = (await response.json()) as Partial<HealthResponse>;
				const nextVersion =
					typeof payload.version === "string"
						? normalizeVersion(payload.version)
						: VERSION_UNKNOWN;
				if (active) {
					setVersion(nextVersion);
				}
			} catch {
				if (active && !abortController.signal.aborted) {
					setVersion(VERSION_UNKNOWN);
				}
			}
		})();

		return () => {
			active = false;
			abortController.abort();
		};
	}, []);

	const currentYear = new Date().getFullYear();

	return (
		<footer className="supports-[backdrop-filter]:bg-background/70 bg-background/95 fixed inset-x-0 bottom-0 z-30 border-t backdrop-blur">
			<div className="mx-auto flex min-h-12 max-w-6xl flex-wrap items-center justify-between gap-x-4 gap-y-1 px-6 py-2 text-xs">
				<span className="text-muted-foreground font-mono">
					© {currentYear} Ivan Li
				</span>
				<a
					className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 font-mono underline-offset-4 hover:underline"
					href={REPOSITORY_URL}
					target="_blank"
					rel="noreferrer"
				>
					<Github className="size-3.5" aria-hidden="true" />
					GitHub
				</a>
				<span className="text-muted-foreground font-mono">
					Version {version}
				</span>
			</div>
		</footer>
	);
}
