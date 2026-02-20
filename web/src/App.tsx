import { useCallback, useEffect, useMemo, useState } from "react";

import { ApiError, apiGet, apiPost } from "@/api";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

type MeResponse = {
	user: {
		id: number;
		github_user_id: number;
		login: string;
		name: string | null;
		avatar_url: string | null;
		email: string | null;
	};
};

type StarredRepoItem = {
	repo_id: number;
	full_name: string;
	description: string | null;
	html_url: string;
	stargazed_at: string | null;
	is_private: number;
};

type ReleaseItem = {
	full_name: string;
	tag_name: string;
	name: string | null;
	published_at: string | null;
	html_url: string;
	is_prerelease: number;
	is_draft: number;
};

type NotificationItem = {
	thread_id: string;
	repo_full_name: string | null;
	subject_title: string | null;
	subject_type: string | null;
	reason: string | null;
	updated_at: string | null;
	unread: number;
	html_url: string | null;
};

type BriefItem = {
	date: string;
	content_markdown: string;
	created_at: string;
};

type SyncStarredResult = { repos: number };
type SyncReleasesResult = { repos: number; releases: number };
type SyncNotificationsResult = { notifications: number; since: string | null };
type BriefGenerateResponse = { content_markdown: string };

function formatIsoShort(iso: string | null) {
	if (!iso) return "";
	return iso.replace("T", " ").replace("Z", "");
}

function App() {
	const [me, setMe] = useState<MeResponse | null>(null);
	const [bootError, setBootError] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);

	const [starred, setStarred] = useState<StarredRepoItem[]>([]);
	const [releases, setReleases] = useState<ReleaseItem[]>([]);
	const [notifications, setNotifications] = useState<NotificationItem[]>([]);
	const [briefs, setBriefs] = useState<BriefItem[]>([]);
	const [generatedBrief, setGeneratedBrief] = useState<string | null>(null);

	const isLoggedIn = Boolean(me?.user?.id);

	const refreshAll = useCallback(async () => {
		if (!isLoggedIn) return;
		const [repos, rels, notifs, b] = await Promise.all([
			apiGet<StarredRepoItem[]>("/api/starred"),
			apiGet<ReleaseItem[]>("/api/releases"),
			apiGet<NotificationItem[]>("/api/notifications"),
			apiGet<BriefItem[]>("/api/briefs"),
		]);
		setStarred(repos);
		setReleases(rels);
		setNotifications(notifs);
		setBriefs(b);
	}, [isLoggedIn]);

	useEffect(() => {
		(async () => {
			try {
				const res = await apiGet<MeResponse>("/api/me");
				setMe(res);
			} catch (err) {
				if (err instanceof ApiError && err.status === 401) {
					setMe(null);
					return;
				}
				setBootError(err instanceof Error ? err.message : String(err));
			}
		})();
	}, []);

	useEffect(() => {
		void refreshAll();
	}, [refreshAll]);

	const latestBrief = useMemo(() => briefs[0] ?? null, [briefs]);

	const run = useCallback(async <T,>(label: string, fn: () => Promise<T>) => {
		setBusy(label);
		setBootError(null);
		try {
			const result = await fn();
			return result;
		} catch (err) {
			setBootError(err instanceof Error ? err.message : String(err));
			throw err;
		} finally {
			setBusy(null);
		}
	}, []);

	if (!isLoggedIn) {
		return (
			<div className="min-h-screen">
				<div className="mx-auto max-w-3xl px-6 py-10">
					<div className="mb-6">
						<h1 className="text-3xl font-semibold tracking-tight">OctoRill</h1>
						<p className="text-muted-foreground mt-2">
							Starred releases + Notifications + AI daily brief.
						</p>
					</div>

					<Card>
						<CardHeader>
							<CardTitle>Sign in</CardTitle>
							<CardDescription>
								Login via GitHub OAuth to sync your starred repos and inbox.
							</CardDescription>
						</CardHeader>
						<CardContent className="flex flex-col gap-3">
							<Button asChild>
								<a href="/auth/github/login">Login with GitHub</a>
							</Button>
							{bootError ? (
								<p className="text-destructive text-sm">{bootError}</p>
							) : null}
							<p className="text-muted-foreground text-xs">
								Tip: in dev, Vite proxies <code>/api</code> and{" "}
								<code>/auth</code> to the Rust backend.
							</p>
						</CardContent>
					</Card>
				</div>
			</div>
		);
	}

	return (
		<div className="min-h-screen">
			<div className="mx-auto max-w-6xl px-6 py-10">
				<div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
					<div>
						<h1 className="text-3xl font-semibold tracking-tight">OctoRill</h1>
						<p className="text-muted-foreground mt-2">
							Logged in as{" "}
							<span className="text-foreground font-medium">
								{me?.user?.login}
							</span>
							.
						</p>
					</div>
					<div className="flex flex-wrap gap-2">
						<Button
							variant="secondary"
							disabled={Boolean(busy)}
							onClick={() => void refreshAll()}
						>
							Refresh
						</Button>
						<Button asChild variant="outline">
							<a href="/auth/logout">Logout</a>
						</Button>
					</div>
				</div>

				{bootError ? (
					<p className="text-destructive mt-4 text-sm">{bootError}</p>
				) : null}

				<div className="mt-6 grid gap-4 md:grid-cols-2">
					<Card>
						<CardHeader>
							<CardTitle>Sync</CardTitle>
							<CardDescription>Pull data from GitHub APIs.</CardDescription>
						</CardHeader>
						<CardContent className="flex flex-col gap-2">
							<Button
								disabled={Boolean(busy)}
								onClick={() =>
									void run("Sync starred", async () => {
										const res =
											await apiPost<SyncStarredResult>("/api/sync/starred");
										await refreshAll();
										return res;
									})
								}
							>
								{busy === "Sync starred" ? "Syncing..." : "Sync starred"}
							</Button>
							<Button
								disabled={Boolean(busy)}
								onClick={() =>
									void run("Sync releases", async () => {
										const res =
											await apiPost<SyncReleasesResult>("/api/sync/releases");
										await refreshAll();
										return res;
									})
								}
							>
								{busy === "Sync releases" ? "Syncing..." : "Sync releases"}
							</Button>
							<Button
								disabled={Boolean(busy)}
								onClick={() =>
									void run("Sync notifications", async () => {
										const res = await apiPost<SyncNotificationsResult>(
											"/api/sync/notifications",
										);
										await refreshAll();
										return res;
									})
								}
							>
								{busy === "Sync notifications" ? "Syncing..." : "Sync inbox"}
							</Button>
						</CardContent>
					</Card>

					<Card>
						<CardHeader>
							<CardTitle>AI Brief</CardTitle>
							<CardDescription>
								Generate a daily brief from the last 24h releases + unread
								inbox.
							</CardDescription>
						</CardHeader>
						<CardContent className="flex flex-col gap-2">
							<Button
								disabled={Boolean(busy)}
								onClick={() =>
									void run("Generate brief", async () => {
										const res = await apiPost<BriefGenerateResponse>(
											"/api/briefs/generate",
										);
										setGeneratedBrief(res.content_markdown);
										await refreshAll();
										return res;
									})
								}
							>
								{busy === "Generate brief" ? "Generating..." : "Generate now"}
							</Button>
							<div className="text-muted-foreground text-xs">
								Latest:{" "}
								<span className="text-foreground">
									{latestBrief ? latestBrief.date : "(none)"}
								</span>
							</div>
						</CardContent>
					</Card>
				</div>

				<div className="mt-6 grid gap-4 lg:grid-cols-3">
					<Card className="lg:col-span-1">
						<CardHeader>
							<CardTitle>Starred</CardTitle>
							<CardDescription>{starred.length} repos</CardDescription>
						</CardHeader>
						<CardContent className="space-y-3">
							{starred.slice(0, 20).map((r) => (
								<div key={r.repo_id} className="space-y-1">
									<a
										className="text-sm font-medium hover:underline"
										href={r.html_url}
										target="_blank"
										rel="noreferrer"
									>
										{r.full_name}
									</a>
									{r.description ? (
										<div className="text-muted-foreground text-xs">
											{r.description}
										</div>
									) : null}
								</div>
							))}
							{starred.length > 20 ? (
								<div className="text-muted-foreground text-xs">
									Showing 20 / {starred.length}
								</div>
							) : null}
						</CardContent>
					</Card>

					<Card className="lg:col-span-1">
						<CardHeader>
							<CardTitle>Releases</CardTitle>
							<CardDescription>Most recent 200</CardDescription>
						</CardHeader>
						<CardContent className="space-y-3">
							{releases.slice(0, 20).map((r) => (
								<div key={`${r.full_name}:${r.tag_name}`} className="space-y-1">
									<a
										className="text-sm font-medium hover:underline"
										href={r.html_url}
										target="_blank"
										rel="noreferrer"
									>
										{r.full_name} — {r.name ?? r.tag_name}
									</a>
									<div className="text-muted-foreground text-xs">
										{formatIsoShort(r.published_at)}
										{r.is_prerelease ? " · prerelease" : ""}
										{r.is_draft ? " · draft" : ""}
									</div>
								</div>
							))}
							{releases.length > 20 ? (
								<div className="text-muted-foreground text-xs">
									Showing 20 / {releases.length}
								</div>
							) : null}
						</CardContent>
					</Card>

					<Card className="lg:col-span-1">
						<CardHeader>
							<CardTitle>Inbox</CardTitle>
							<CardDescription>{notifications.length} threads</CardDescription>
						</CardHeader>
						<CardContent className="space-y-3">
							{notifications.slice(0, 20).map((n) => (
								<div key={n.thread_id} className="space-y-1">
									<div className="text-sm font-medium">
										{n.repo_full_name ?? "(unknown repo)"}
									</div>
									<div className="text-muted-foreground text-xs">
										{n.subject_title ?? "(no title)"}
										{n.reason ? ` · ${n.reason}` : ""}
										{n.updated_at ? ` · ${formatIsoShort(n.updated_at)}` : ""}
									</div>
								</div>
							))}
							{notifications.length > 20 ? (
								<div className="text-muted-foreground text-xs">
									Showing 20 / {notifications.length}
								</div>
							) : null}
						</CardContent>
					</Card>
				</div>

				{generatedBrief || latestBrief ? (
					<div className="mt-6">
						<Card>
							<CardHeader>
								<CardTitle>Brief (Markdown)</CardTitle>
								<CardDescription>
									{generatedBrief
										? "Generated just now (not rendered)."
										: `From ${latestBrief?.date}`}
								</CardDescription>
							</CardHeader>
							<CardContent>
								<pre className="bg-muted/30 overflow-auto rounded-md border p-4 text-sm whitespace-pre-wrap">
									{generatedBrief ?? latestBrief?.content_markdown ?? ""}
								</pre>
							</CardContent>
						</Card>
					</div>
				) : null}
			</div>
		</div>
	);
}

export default App;
