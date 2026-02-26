import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";

type LandingProps = {
	bootError?: string | null;
};

export function Landing({ bootError }: LandingProps) {
	return (
		<AppShell footer={<AppMetaFooter />}>
			<div className="mx-auto max-w-3xl py-4 sm:py-8">
				<div className="mb-8">
					<div className="inline-flex items-center gap-2 rounded-full border bg-card px-3 py-1 text-xs font-medium">
						<span className="font-mono text-muted-foreground">OctoRill</span>
						<span className="text-muted-foreground">
							GitHub 信息流 · 中文翻译
						</span>
					</div>

					<h1 className="mt-6 text-balance text-4xl font-semibold tracking-tight">
						把 GitHub 的更新变成可读的信息流
					</h1>
					<p className="text-muted-foreground mt-3 text-pretty leading-relaxed">
						Releases 信息流（无限滚动），AI 自动翻译成中文；并提供“昨日 Release
						日报”和 Inbox 快捷入口。需要操作时，直接跳回 GitHub。
					</p>
				</div>

				<Card className="shadow-sm">
					<CardHeader>
						<CardTitle>登录</CardTitle>
						<CardDescription>
							通过 GitHub OAuth 登录后，才能同步你的 starred 与 inbox。
						</CardDescription>
					</CardHeader>
					<CardContent className="flex flex-col gap-3">
						<Button asChild className="w-full">
							<a href="/auth/github/login">使用 GitHub 登录</a>
						</Button>

						{bootError ? (
							<p className="text-destructive text-sm">{bootError}</p>
						) : null}
					</CardContent>
				</Card>
			</div>
		</AppShell>
	);
}
