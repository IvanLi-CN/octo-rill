import { Compass, Home, LogIn, Settings2 } from "lucide-react";

import { BrandLogo } from "@/components/brand/BrandLogo";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { ThemeToggle } from "@/components/theme/ThemeToggle";

export function NotFoundPage(props: {
	isAuthenticated: boolean;
	pathname?: string | null;
}) {
	const { isAuthenticated, pathname } = props;
	const missingPath = pathname?.trim() ? pathname : null;

	return (
		<AppShell footer={<AppMetaFooter />}>
			<div className="mx-auto max-w-5xl py-2 sm:py-4">
				<div className="mb-4 flex items-center justify-between gap-4 sm:mb-6">
					<BrandLogo variant="wordmark" className="h-8 sm:h-10" />
					<ThemeToggle />
				</div>

				<section
					className="rounded-[32px] border border-border/70 bg-[linear-gradient(180deg,color-mix(in_oklab,var(--color-card)_92%,transparent),color-mix(in_oklab,var(--color-card)_76%,transparent))] p-6 shadow-[0_20px_50px_rgba(15,23,42,0.07)] dark:shadow-[0_28px_60px_rgba(2,6,23,0.42)] sm:p-8 lg:p-10"
					data-not-found-surface
				>
					<div className="max-w-3xl space-y-6">
						<Badge
							variant="outline"
							className="rounded-full border-border/80 px-3 py-1 text-[11px] tracking-[0.18em] uppercase"
						>
							404 · Route Not Found
						</Badge>

						<div className="space-y-4">
							<div className="flex items-center gap-3 text-muted-foreground">
								<Compass className="size-5" />
								<span className="text-sm font-medium">找不到这个页面入口</span>
							</div>
							<h1 className="text-balance text-4xl font-semibold tracking-tight sm:text-5xl">
								页面不存在
							</h1>
							<p className="text-muted-foreground max-w-2xl text-pretty text-base leading-7 sm:text-lg">
								当前 URL 没有命中 OctoRill
								的前端路由。应用壳层仍会正常启动，但这里应该明确告诉你这是一个不存在的页面，而不是静默回到其他界面。
							</p>
							{missingPath ? (
								<div className="rounded-2xl border border-border/70 bg-background/72 px-4 py-3 text-sm leading-6">
									<p className="text-muted-foreground mb-1 text-xs font-medium tracking-[0.16em] uppercase">
										Requested Path
									</p>
									<p className="font-mono text-[13px] text-foreground">
										{missingPath}
									</p>
								</div>
							) : null}
						</div>

						<div className="flex flex-wrap gap-3">
							<Button asChild size="lg" className="rounded-2xl">
								<InternalLink href="/" to="/">
									<Home className="size-4" />
									{isAuthenticated ? "返回工作台" : "回到首页"}
								</InternalLink>
							</Button>
							{isAuthenticated ? (
								<Button
									asChild
									variant="outline"
									size="lg"
									className="rounded-2xl"
								>
									<InternalLink href="/settings" to="/settings">
										<Settings2 className="size-4" />
										打开设置
									</InternalLink>
								</Button>
							) : (
								<Button
									asChild
									variant="outline"
									size="lg"
									className="rounded-2xl"
								>
									<a href="/auth/github/login">
										<LogIn className="size-4" />
										连接到 GitHub
									</a>
								</Button>
							)}
						</div>
					</div>
				</section>
			</div>
		</AppShell>
	);
}
