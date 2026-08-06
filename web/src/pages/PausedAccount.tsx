import { useEffect, useRef, useState } from "react";
import { useRouter } from "@tanstack/react-router";

import { apiResumeAccount } from "@/api";
import {
	PausedAccountActionPanel,
	type PausedAccountActionState,
} from "@/account/PausedAccountActionPanel";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { openAppEventSource } from "@/demo/eventSource";
import { useAuthBootstrap } from "@/auth/AuthBootstrap";

export function PausedAccountPage() {
	const auth = useAuthBootstrap();
	const router = useRouter();
	const [state, setState] = useState<PausedAccountActionState>("idle");
	const [error, setError] = useState<string | null>(null);
	const eventSourceRef = useRef<EventSource | null>(null);

	useEffect(() => () => eventSourceRef.current?.close(), []);

	const goHome = () => {
		eventSourceRef.current?.close();
		void auth.refreshAuth().finally(() => {
			void router.navigate({
				to: "/",
				search: {
					tab: undefined,
					release: undefined,
					from: undefined,
					brief: undefined,
					scope: undefined,
					items: undefined,
					org: undefined,
				},
				replace: true,
			});
		});
	};

	const resume = async () => {
		setError(null);
		setState("resuming");
		eventSourceRef.current?.close();
		try {
			const response = await apiResumeAccount();
			if (response.sync_enqueue_error) {
				setState("enqueue_failed");
				setError("账号已恢复，但访问同步没有入队成功，请重试。");
				return;
			}
			const eventPath = response.access_sync.event_path;
			if (!eventPath) {
				setState("succeeded");
				return;
			}
			setState("queued");
			const source = openAppEventSource(eventPath, { withCredentials: true });
			eventSourceRef.current = source;
			source.addEventListener("task.running", () => setState("syncing"));
			source.addEventListener("task.progress", () => setState("syncing"));
			source.addEventListener("task.completed", (event) => {
				const payload = JSON.parse((event as MessageEvent).data) as {
					status?: string;
					error?: string;
				};
				if (payload.status === "succeeded") {
					setState("succeeded");
					setError(null);
				} else {
					setState("failed");
					setError(payload.error ?? "访问同步未完成，请重试。");
				}
				source.close();
			});
			source.onerror = () => {
				setState("failed");
				setError("访问同步连接中断，请重试。");
				source.close();
			};
		} catch (err) {
			setState("failed");
			setError(err instanceof Error ? err.message : "恢复账号失败，请重试。");
		}
	};

	const login = auth.me?.user.login ?? "当前账号";

	return (
		<AppShell footer={<AppMetaFooter />} mobileChrome={false}>
			<main className="min-h-[calc(100dvh-5rem)] bg-background px-4 py-10 sm:px-6 sm:py-16">
				<div className="mx-auto flex w-full max-w-5xl flex-col gap-10">
					<div className="flex h-8 items-center">
						<BrandLogo variant="wordmark" className="h-7" />
					</div>
					<div className="flex min-h-[55dvh] items-center justify-center">
						<PausedAccountActionPanel
							login={login}
							state={state}
							error={error}
							onResume={() => void resume()}
							onHome={goHome}
							onLogout={() => {
								window.location.assign("/auth/logout");
							}}
						/>
					</div>
				</div>
			</main>
		</AppShell>
	);
}
