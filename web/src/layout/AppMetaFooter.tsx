import { Github } from "lucide-react";

import { useAppShellChrome } from "@/layout/AppShell";
import { useVersionMonitor } from "@/version/versionMonitor";

const REPOSITORY_URL = "https://github.com/IvanLi-CN/octo-rill";

export function AppMetaFooter() {
	const { loadedVersion } = useVersionMonitor();
	const {
		footerHidden,
		mobileChromeEnabled,
		isMobileViewport,
		viewportBottomInset,
	} = useAppShellChrome();
	const currentYear = new Date().getFullYear();

	return (
		<footer
			className={`supports-[backdrop-filter]:bg-background/70 bg-background/95 fixed inset-x-0 bottom-0 z-30 border-t backdrop-blur transition-transform duration-200 ease-out ${
				footerHidden ? "translate-y-full pointer-events-none" : "translate-y-0"
			}`}
			data-app-meta-footer-hidden={footerHidden ? "true" : "false"}
			style={
				mobileChromeEnabled && isMobileViewport
					? {
							bottom: `calc(env(safe-area-inset-bottom, 0px) + ${viewportBottomInset}px)`,
						}
					: undefined
			}
		>
			<div
				className={`mx-auto flex min-h-12 max-w-6xl flex-wrap items-center justify-between gap-x-4 gap-y-1 px-6 py-2 text-xs ${
					mobileChromeEnabled && isMobileViewport ? "px-4" : ""
				}`}
			>
				<span className="text-muted-foreground font-mono">
					© {currentYear} Ivan Li
				</span>
				<div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
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
			</div>
		</footer>
	);
}
