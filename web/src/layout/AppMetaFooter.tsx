import { Github } from "lucide-react";
import { useEffect, useState } from "react";

type HealthResponse = {
	ok: boolean;
	version: string;
};

type VersionResponse = {
	ok: boolean;
	version: string;
	source: string;
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

async function fetchVersionFromEndpoint(
	endpoint: string,
	signal: AbortSignal,
): Promise<string> {
	const response = await fetch(endpoint, {
		credentials: "include",
		signal,
	});
	if (!response.ok) {
		throw new Error(`version request failed (${endpoint}): ${response.status}`);
	}

	const payload = (await response.json()) as
		| Partial<HealthResponse>
		| Partial<VersionResponse>;
	if (typeof payload.version !== "string") {
		throw new Error(`version payload missing version field (${endpoint})`);
	}
	return normalizeVersion(payload.version);
}

async function fetchVersion(signal: AbortSignal): Promise<string> {
	try {
		return await fetchVersionFromEndpoint("/api/version", signal);
	} catch {
		return fetchVersionFromEndpoint("/api/health", signal);
	}
}

export function AppMetaFooter() {
	const [version, setVersion] = useState(VERSION_LOADING);

	useEffect(() => {
		const abortController = new AbortController();
		let active = true;

		void (async () => {
			try {
				const nextVersion = await fetchVersion(abortController.signal);
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
