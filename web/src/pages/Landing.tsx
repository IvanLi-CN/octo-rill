import { Button } from "@/components/ui/button";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

type LandingProps = {
	bootError?: string | null;
};

const heroHighlights = [
	{
		title: "Release Feed",
		description: "把 starred 仓库的更新整理成更适合连续阅读的信息流。",
	},
	{
		title: "AI 中文翻译",
		description: "自动生成中文摘要，先快速理解，再决定是否回到原文。",
	},
	{
		title: "Inbox 工作台",
		description: "把日报、通知和待处理入口收进同一个首页起点。",
	},
] as const;

export function Landing({ bootError }: LandingProps) {
	return (
		<AppShell notice={<VersionUpdateNotice />} footer={<AppMetaFooter />}>
			<div className="mx-auto max-w-6xl py-4 sm:py-8">
				<div className="mb-4 flex items-center justify-end sm:mb-6">
					<ThemeToggle />
				</div>

				<div className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_360px] lg:items-start xl:grid-cols-[minmax(0,1fr)_400px] xl:gap-10">
					<section className="space-y-6">
						<div className="rounded-[32px] border border-[#E4DACB] bg-[linear-gradient(135deg,rgba(255,255,255,0.96),rgba(255,248,238,0.96),rgba(246,239,229,0.92))] p-6 shadow-[0_20px_50px_rgba(40,28,12,0.06)] dark:border-[#2d3645] dark:bg-[linear-gradient(135deg,rgba(20,27,37,0.96),rgba(14,20,31,0.98),rgba(9,13,21,0.98))] dark:shadow-[0_24px_60px_rgba(2,6,23,0.45)] sm:p-8">
							<div className="flex flex-col gap-8">
								<div className="inline-flex w-fit flex-col gap-3 rounded-[28px] bg-white/92 px-5 py-5 shadow-sm shadow-black/5 ring-1 ring-[#E9DECF] dark:bg-[#131926]/92 dark:ring-[#344055] dark:shadow-[0_12px_35px_rgba(2,6,23,0.35)]">
									<BrandLogo variant="wordmark" className="h-10 sm:h-11" />
									<p className="text-muted-foreground text-sm font-medium sm:text-base">
										GitHub 信息流 · AI 中文翻译 · Inbox 工作台
									</p>
								</div>

								<div className="max-w-3xl space-y-4">
									<div className="inline-flex items-center rounded-full border border-[#E5D9C7] bg-white/70 px-3 py-1 text-sm font-medium text-[#6B5C4A] dark:border-[#334155] dark:bg-[#111827]/70 dark:text-[#dbe7ff]">
										为 GitHub Release 阅读而生
									</div>
									<h1 className="text-balance text-4xl font-semibold tracking-tight sm:text-5xl lg:text-6xl">
										把 GitHub 的更新变成可读的信息流
									</h1>
									<p className="text-muted-foreground max-w-2xl text-pretty text-lg leading-relaxed">
										Releases 信息流（无限滚动），AI 自动翻译成中文；并提供“昨日
										Release 日报”和 Inbox 快捷入口。需要操作时，直接跳回
										GitHub。
									</p>
								</div>

								<div className="grid gap-3 sm:grid-cols-3">
									{heroHighlights.map((item) => (
										<div
											key={item.title}
											className="rounded-[24px] border border-[#E8DDCF] bg-white/72 px-4 py-4 shadow-sm shadow-black/5 dark:border-[#344055] dark:bg-[#121926]/78 dark:shadow-[0_12px_32px_rgba(2,6,23,0.35)]"
										>
											<p className="text-base font-semibold text-[#3A3329] dark:text-[#f8fafc]">
												{item.title}
											</p>
											<p className="text-muted-foreground mt-2 text-sm leading-relaxed">
												{item.description}
											</p>
										</div>
									))}
								</div>
							</div>
						</div>
					</section>

					<aside className="lg:pt-4">
						<Card className="rounded-[32px] border-[#E4DACB] bg-card/95 shadow-[0_18px_45px_rgba(40,28,12,0.08)] dark:border-[#2d3645] dark:bg-card/96 dark:shadow-[0_24px_55px_rgba(2,6,23,0.42)]">
							<CardHeader className="gap-4 p-7">
								<div className="inline-flex w-fit items-center rounded-full bg-[#FFF3E3] px-3 py-1 text-xs font-semibold tracking-[0.18em] text-[#7A6247] uppercase dark:bg-[#1e293b] dark:text-[#f7d9a8]">
									Start here
								</div>
								<div className="space-y-2">
									<CardTitle className="text-3xl">登录</CardTitle>
									<CardDescription className="text-base leading-relaxed">
										通过 GitHub OAuth 登录后，OctoRill 才能同步你的 starred 与
										inbox，并把你真正关心的更新带进首页。
									</CardDescription>
								</div>
							</CardHeader>
							<CardContent className="flex flex-col gap-4 p-7 pt-0">
								<div className="grid gap-3">
									<div className="rounded-2xl border border-[#E8DDCF] bg-[#FFF8EE] px-4 py-3 dark:border-[#344055] dark:bg-[#172030]">
										<p className="text-sm font-semibold text-[#3A3329] dark:text-[#f8fafc]">
											同步 starred 与 inbox
										</p>
										<p className="text-muted-foreground mt-1 text-sm leading-relaxed">
											自动聚合你已经关注的仓库和通知，不需要重新组织信息源。
										</p>
									</div>
									<div className="rounded-2xl border border-[#D9E7DE] bg-[#F2FBF7] px-4 py-3 dark:border-[#28463d] dark:bg-[#11221f]">
										<p className="text-sm font-semibold text-[#3A3329] dark:text-[#f8fafc]">
											仍然回到 GitHub 操作
										</p>
										<p className="text-muted-foreground mt-1 text-sm leading-relaxed">
											阅读、筛选和理解在这里完成；真正的点赞、评论和 Issue
											仍然回到原生页面。
										</p>
									</div>
								</div>

								<Button
									asChild
									className="h-14 w-full rounded-2xl text-base font-semibold"
								>
									<a href="/auth/github/login">使用 GitHub 登录</a>
								</Button>

								{bootError ? (
									<div className="rounded-2xl border border-destructive/20 bg-destructive/5 px-4 py-3 text-sm text-destructive">
										{bootError}
									</div>
								) : null}
							</CardContent>
						</Card>
					</aside>
				</div>
			</div>
		</AppShell>
	);
}
