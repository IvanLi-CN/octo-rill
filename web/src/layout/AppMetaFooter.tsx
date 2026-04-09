import { Github } from "lucide-react";

import { useVersionMonitor } from "@/version/versionMonitor";

const REPOSITORY_URL = "https://github.com/IvanLi-CN/octo-rill";

export function AppMetaFooter() {
	const { loadedVersion } = useVersionMonitor();
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
					Version {loadedVersion}
				</span>
			</div>
		</footer>
	);
}
