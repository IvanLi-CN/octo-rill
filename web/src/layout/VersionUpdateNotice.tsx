import { RefreshCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useVersionMonitor } from "@/version/versionMonitor";

export function VersionUpdateNotice() {
	const { availableVersion, hasUpdate, refreshPage } = useVersionMonitor();

	if (!hasUpdate || !availableVersion) {
		return null;
	}

	return (
		<div
			className="border-b border-amber-200/70 bg-amber-50/80 dark:border-amber-500/25 dark:bg-amber-500/10"
			data-version-update-notice
		>
			<div className="mx-auto flex min-h-10 max-w-6xl items-center justify-between gap-3 px-6 py-2">
				<p className="text-[13px] text-amber-950/85 dark:text-amber-100/90 sm:text-sm">
					检测到新版本{" "}
					<span className="font-mono font-medium">{availableVersion}</span>
					，按需刷新即可。
				</p>
				<Button
					type="button"
					variant="ghost"
					size="sm"
					className="h-7 px-2.5 text-xs text-amber-950 hover:bg-amber-100 hover:text-amber-950 dark:text-amber-50 dark:hover:bg-amber-500/20 dark:hover:text-amber-50"
					onClick={refreshPage}
				>
					<RefreshCcw className="size-3.5" />
					刷新
				</Button>
			</div>
		</div>
	);
}
