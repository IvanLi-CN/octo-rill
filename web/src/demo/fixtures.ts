import type { AdminUserItem } from "@/admin/UserManagement";
import type {
	AdminLlmCallDetailResponse,
	AdminLlmCallItem,
	AdminRealtimeTaskDetailResponse,
	AdminRealtimeTaskItem,
	AdminTaskDiagnostics,
	AdminUserProfileResponse,
	ApiKeySummary,
	FollowingReposResponse,
	GitHubConnectionResponse,
	MeLinuxDoResponse,
	MeProfileResponse,
	MeResponse,
	PasskeySummary,
	ReactionTokenStatusResponse,
	ReleaseDetailResponse,
	RepoPublicReleasePublicationStatusResponse,
} from "@/api";
import type { FeedItem, FeedResponse } from "@/feed/types";
import type { RepoVisual } from "@/lib/repoVisual";
import type { NotificationItem } from "@/sidebar/InboxQuickList";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";
import type {
	DemoJobsModel,
	DemoModel,
	DemoPersonaId,
	DemoPublicationState,
} from "@/demo/types";

function svgAvatarDataUrl(
	label: string,
	background: string,
	foreground = "#ffffff",
) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="120" fill="${background}"/><text x="120" y="132" font-family="ui-sans-serif, system-ui, sans-serif" font-size="52" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function buildRepoVisual(ownerLabel: string, color: string): RepoVisual {
	return {
		owner_avatar_url: svgAvatarDataUrl(ownerLabel, color),
		open_graph_image_url: null,
		uses_custom_open_graph_image: false,
	};
}

const NOW = "2026-07-08T10:30:00+08:00";
const USER_ID = "demo-user-01";
const ADMIN_USER_ID = "demo-admin-01";
const OWNER_SCOPE = { owner: "octo-demo", repo: "release-lab" } as const;
const OWNER_REPO_FULL_NAME = `${OWNER_SCOPE.owner}/${OWNER_SCOPE.repo}`;
const OWNER_REPO_VISUAL = buildRepoVisual("OR", "#0f4c81");
const DOCS_SCOPE = { owner: "octo-demo", repo: "docs-hub" } as const;
const DOCS_REPO_FULL_NAME = `${DOCS_SCOPE.owner}/${DOCS_SCOPE.repo}`;
const DOCS_REPO_VISUAL = buildRepoVisual("DS", "#0f766e");

function buildMe(
	personaId: DemoPersonaId,
	includeOwnReleases: boolean,
): MeResponse | null {
	if (personaId === "guest") {
		return null;
	}

	const isAdmin = personaId === "admin";

	return {
		user: {
			id: isAdmin ? ADMIN_USER_ID : USER_ID,
			github_user_id: isAdmin ? 1 : 2,
			login: isAdmin ? "octo-admin" : "octo-member",
			name: isAdmin ? "Octo Admin" : "Octo Member",
			avatar_url: svgAvatarDataUrl(
				isAdmin ? "AD" : "ME",
				isAdmin ? "#9f1239" : "#1d4ed8",
			),
			email: isAdmin ? "admin@octo.demo" : "member@octo.demo",
			is_admin: isAdmin,
		},
		access_sync: {
			task_id: null,
			task_type: null,
			event_path: null,
			reason: "none",
		},
		dashboard: {
			daily_boundary_local: "08:00",
			daily_boundary_time_zone: "Asia/Shanghai",
			daily_boundary_utc_offset_minutes: 480,
			include_own_releases: includeOwnReleases,
		},
	};
}

function buildProfile(
	me: MeResponse | null,
	includeOwnReleases: boolean,
): MeProfileResponse {
	return {
		user_id: me?.user.id ?? USER_ID,
		daily_brief_local_time: "08:00",
		daily_brief_time_zone: "Asia/Shanghai",
		include_own_releases: includeOwnReleases,
		last_active_at: "2026-07-08T09:58:00+08:00",
	};
}

function buildGitHubConnections(): GitHubConnectionResponse[] {
	return [
		{
			id: "gh-primary",
			github_user_id: 2,
			login: "octo-demo-owner",
			name: "Octo Demo Owner",
			avatar_url: svgAvatarDataUrl("IV", "#111827"),
			email: "owner@octo.demo",
			scopes: "read:user,user:email,notifications,public_repo",
			linked_at: "2026-07-06T09:12:00+08:00",
			updated_at: "2026-07-08T09:30:00+08:00",
		},
		{
			id: "gh-ops",
			github_user_id: 5,
			login: "octo-ops",
			name: "Octo Ops",
			avatar_url: svgAvatarDataUrl("OP", "#0f766e"),
			email: "ops@octo.demo",
			scopes: "read:user,user:email,notifications,public_repo",
			linked_at: "2026-07-05T08:30:00+08:00",
			updated_at: "2026-07-08T09:31:00+08:00",
		},
	];
}

function buildLinuxDo(): MeLinuxDoResponse {
	return {
		available: true,
		connection: {
			linuxdo_user_id: 9527,
			username: "octo-linuxdo",
			name: "Octo LinuxDO",
			avatar_url: svgAvatarDataUrl("LD", "#ca8a04"),
			trust_level: 3,
			active: true,
			silenced: false,
			linked_at: "2026-07-05T15:00:00+08:00",
			updated_at: "2026-07-08T08:40:00+08:00",
		},
	};
}

function buildPasskeys(): PasskeySummary[] {
	return [
		{
			id: "passkey-macbook",
			label: "MacBook Pro Touch ID",
			created_at: "2026-07-03T18:00:00+08:00",
			last_used_at: "2026-07-08T09:00:00+08:00",
		},
		{
			id: "passkey-yubikey",
			label: "YubiKey 5C NFC",
			created_at: "2026-07-02T20:00:00+08:00",
			last_used_at: null,
		},
	];
}

function buildApiKeys(): ApiKeySummary[] {
	return [
		{
			id: "api-key-demo-1",
			name: "Demo automation",
			api_key: "orill_demo_key_not_real_plaintext",
			masked_key: "orill_demo_key...ckup",
			created_at: "2026-07-01T10:00:00+08:00",
			last_used_at: "2026-07-08T09:25:00+08:00",
		},
	];
}

function buildReactionToken(): ReactionTokenStatusResponse {
	return {
		configured: true,
		masked_token: "demo_pat_token_xxxxxxxx",
		owner: {
			github_connection_id: "gh-primary",
			github_user_id: 2,
			login: "octo-demo-owner",
		},
		check: {
			state: "valid",
			message: "PAT 可用",
			checked_at: "2026-07-08T09:12:00+08:00",
		},
	};
}

function buildFeed(includeOwnReleases: boolean): FeedResponse {
	const items: FeedItem[] = [
		{
			kind: "release",
			id: "release-owner-1",
			ts: "2026-07-08T09:20:00+08:00",
			repo_full_name: OWNER_REPO_FULL_NAME,
			repo_visual: OWNER_REPO_VISUAL,
			title: "v2.31.0 web demo contract",
			body: "把 `/demo/` 收口成公开页面级验收面，并统一 inspector / share / simulated write 合同。",
			body_truncated: false,
			subtitle: "owner-only release · mock publish controls",
			reason: "release",
			subject_type: "Release",
			html_url: "https://github.com/octo-demo/release-lab/releases/tag/v2.31.0",
			unread: null,
			actor: null,
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "v2.31.0 Web Demo 合同",
				summary:
					"公开 `/demo/`、MSW mock-only runtime、结构化 inspector 与 simulated writes。",
			},
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "Web Demo 作为页面验收主来源",
				summary:
					"该版本把页面级视觉证据、深链分享与 mock-only 写操作统一到 `/demo/` 子应用。",
			},
			reactions: {
				status: "ready",
				counts: {
					plus1: 4,
					laugh: 0,
					heart: 2,
					hooray: 1,
					rocket: 3,
					eyes: 1,
				},
				viewer: {
					plus1: false,
					laugh: false,
					heart: true,
					hooray: false,
					rocket: false,
					eyes: false,
				},
			},
		},
		{
			kind: "release",
			id: "release-public-2",
			ts: "2026-07-08T08:50:00+08:00",
			repo_full_name: DOCS_REPO_FULL_NAME,
			repo_visual: DOCS_REPO_VISUAL,
			title: "v1.12.0 pages docs refresh",
			body: "公开文档新增 `/demo/` 入口与 404 deep-link recovery 说明。",
			body_truncated: false,
			subtitle: "public repo",
			reason: "release",
			subject_type: "Release",
			html_url: "https://github.com/octo-demo/docs-hub/releases/tag/v1.12.0",
			unread: null,
			actor: null,
			translated: {
				lang: "zh-CN",
				status: "ready",
				title: "Pages 文档刷新",
				summary: "docs-site 现在会公开引导到 `/demo/` 页面级 demo。",
			},
			smart: {
				lang: "zh-CN",
				status: "ready",
				title: "公开入口补齐",
				summary: "文档站加入 demo 入口，避免组件 QA 和页面证据混用。",
			},
			reactions: {
				status: "ready",
				counts: {
					plus1: 2,
					laugh: 0,
					heart: 0,
					hooray: 1,
					rocket: 1,
					eyes: 0,
				},
				viewer: {
					plus1: true,
					laugh: false,
					heart: false,
					hooray: false,
					rocket: false,
					eyes: false,
				},
			},
		},
		{
			kind: "repo_star_received",
			id: "star-1",
			ts: "2026-07-08T08:10:00+08:00",
			repo_full_name: OWNER_REPO_FULL_NAME,
			repo_visual: OWNER_REPO_VISUAL,
			title: "Demo Scout 为 OctoRill 点了星",
			body: null,
			body_truncated: false,
			subtitle: "story-driven demo evidence",
			reason: "star",
			subject_type: "Star",
			html_url: "https://github.com/octo-demo/release-lab/stargazers",
			unread: null,
			actor: {
				login: "demo-scout",
				avatar_url: svgAvatarDataUrl("DS", "#9333ea"),
				html_url: "https://github.com/octo-demo-scout",
			},
			translated: null,
			smart: null,
			reactions: null,
		},
	];

	return {
		items: includeOwnReleases
			? items
			: items.filter((item) => item.id !== "release-owner-1"),
		next_cursor: null,
	};
}

function buildBriefs(): BriefItem[] {
	return [
		{
			id: "brief-2026-07-08",
			date: "2026-07-08",
			window_start: "2026-07-07T08:00:00+08:00",
			window_end: "2026-07-08T08:00:00+08:00",
			effective_time_zone: "Asia/Shanghai",
			effective_local_boundary: "08:00",
			release_count: 2,
			release_ids: ["release-owner-1", "release-public-2"],
			preview_markdown:
				"- 新增 `/demo/` Web Demo 子应用\n- Settings / Admin 页面支持 simulated writes\n",
			content_markdown:
				"## 今日摘要\n\n- 新增公开 `/demo/` 页面级 demo。\n- Dashboard repo scope 支持 simulated publish / unpublish。\n- Admin Jobs 的运行态与设置保存进入 mock-only contract。\n",
			created_at: "2026-07-08T08:06:00+08:00",
			updated_at: "2026-07-08T08:12:00+08:00",
		},
		{
			id: "brief-2026-07-07",
			date: "2026-07-07",
			window_start: "2026-07-06T08:00:00+08:00",
			window_end: "2026-07-07T08:00:00+08:00",
			effective_time_zone: "Asia/Shanghai",
			effective_local_boundary: "08:00",
			release_count: 1,
			release_ids: ["release-public-2"],
			preview_markdown: "- 补齐 docs-site 与 Storybook 的职责分界\n",
			content_markdown: null,
			created_at: "2026-07-07T08:04:00+08:00",
			updated_at: "2026-07-07T08:05:00+08:00",
		},
	];
}

function buildNotifications(): NotificationItem[] {
	return [
		{
			thread_id: "thread-1",
			repo_full_name: OWNER_REPO_FULL_NAME,
			subject_title: "PR #207: Web demo inspector contract",
			subject_type: "PullRequest",
			reason: "review_requested",
			updated_at: "2026-07-08T09:26:00+08:00",
			unread: 2,
			html_url: "https://github.com/octo-demo/release-lab/pull/207",
		},
		{
			thread_id: "thread-2",
			repo_full_name: DOCS_REPO_FULL_NAME,
			subject_title: "Pages artifact updated for /demo/",
			subject_type: "CheckSuite",
			reason: "subscribed",
			updated_at: "2026-07-08T08:44:00+08:00",
			unread: 0,
			html_url: "https://github.com/octo-demo/docs-hub/actions",
		},
	];
}

function buildFollowingRepos(): FollowingReposResponse {
	return {
		following_count: 2,
		associated_count: 3,
		items: [
			{
				repo_id: 1001,
				full_name: OWNER_REPO_FULL_NAME,
				owner_login: OWNER_SCOPE.owner,
				name: OWNER_SCOPE.repo,
				html_url: "https://github.com/octo-demo/release-lab",
				description: "Fictional private repo for mock-only acceptance.",
				is_private: true,
				first_source: "personal_owned",
				first_associated_at: "2026-07-01T10:00:00+08:00",
				last_seen_at: NOW,
				is_following: true,
				follow_state_source: "manual_feed",
				repo_visual: OWNER_REPO_VISUAL,
				sources: {
					personal_owned: true,
					github_star: false,
					manual_feed: true,
				},
			},
			{
				repo_id: 1002,
				full_name: DOCS_REPO_FULL_NAME,
				owner_login: DOCS_SCOPE.owner,
				name: DOCS_SCOPE.repo,
				html_url: "https://github.com/octo-demo/docs-hub",
				description: "Public docs and Pages assembly",
				is_private: false,
				first_source: "github_star",
				first_associated_at: "2026-07-03T10:00:00+08:00",
				last_seen_at: NOW,
				is_following: true,
				follow_state_source: "github_star",
				repo_visual: DOCS_REPO_VISUAL,
				sources: {
					personal_owned: false,
					github_star: true,
					manual_feed: false,
				},
			},
		],
		associated_items: [],
	};
}

function buildPublicReleaseDetail(): ReleaseDetailResponse {
	return {
		release_id: "291058027",
		repo_full_name: OWNER_REPO_FULL_NAME,
		repo_visual: OWNER_REPO_VISUAL,
		tag_name: "v2.31.0",
		previous_tag_name: "v2.30.2",
		name: "v2.31.0 web demo contract",
		body: "## Changes\n\n- Public `/demo/` sub app\n- Shared MSW scenes\n- Floating inspector with bubble + edge snap\n",
		html_url: "https://github.com/octo-demo/release-lab/releases/tag/v2.31.0",
		published_at: "2026-07-08T08:00:00Z",
		is_prerelease: 0,
		is_draft: 0,
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: "v2.31.0 Web Demo 合同",
			summary:
				"这次版本把页面级 demo、公开 `/demo/` 路径与 inspector 行为正式冻结下来。",
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "页面证据与组件 QA 分流",
			summary:
				"Storybook 继续负责组件/局部状态，最终页面证据转到 mock-only `/demo/` 子应用。",
		},
	};
}

function buildPublicationStatus(
	publicationState: DemoPublicationState,
): RepoPublicReleasePublicationStatusResponse {
	return {
		repo_full_name: OWNER_REPO_FULL_NAME,
		public_path: `/public/${OWNER_SCOPE.owner}/${OWNER_SCOPE.repo}/releases`,
		visibility: "private",
		publication_state:
			publicationState === "published"
				? "private_owner_published"
				: "private_owner_unpublished",
		can_publish: publicationState !== "published",
		can_unpublish: publicationState === "published",
		last_sync_status: "ready",
		published_at:
			publicationState === "published" ? "2026-07-08T09:05:00+08:00" : null,
		reason: null,
		cache_cleanup: null,
	};
}

function buildAdminUsers(): AdminUserItem[] {
	return [
		{
			id: ADMIN_USER_ID,
			github_user_id: 1,
			login: "octo-admin",
			name: "Octo Admin",
			avatar_url: svgAvatarDataUrl("AD", "#9f1239"),
			email: "admin@octo.demo",
			is_admin: true,
			is_disabled: false,
			repo_total: 48,
			include_own_releases: true,
			last_active_at: "2026-07-08T09:58:00+08:00",
			created_at: "2026-06-01T09:00:00+08:00",
			updated_at: "2026-07-08T09:58:00+08:00",
		},
		{
			id: USER_ID,
			github_user_id: 2,
			login: "octo-member",
			name: "Octo Member",
			avatar_url: svgAvatarDataUrl("ME", "#1d4ed8"),
			email: "member@octo.demo",
			is_admin: false,
			is_disabled: false,
			repo_total: 21,
			include_own_releases: false,
			last_active_at: "2026-07-08T09:26:00+08:00",
			created_at: "2026-06-04T12:00:00+08:00",
			updated_at: "2026-07-08T09:26:00+08:00",
		},
		{
			id: "disabled-user",
			github_user_id: 9,
			login: "octo-paused",
			name: "Paused Member",
			avatar_url: null,
			email: "paused@octo.demo",
			is_admin: false,
			is_disabled: true,
			repo_total: 3,
			include_own_releases: false,
			last_active_at: null,
			created_at: "2026-05-20T12:00:00+08:00",
			updated_at: "2026-07-06T11:20:00+08:00",
		},
	];
}

function buildAdminUserProfiles(
	users: AdminUserItem[],
): Record<string, AdminUserProfileResponse> {
	return Object.fromEntries(
		users.map((user) => [
			user.id,
			{
				user_id: user.id,
				daily_brief_local_time: user.id === "disabled-user" ? "09:00" : "08:00",
				daily_brief_time_zone: "Asia/Shanghai",
				include_own_releases: user.include_own_releases,
				last_active_at: user.last_active_at,
			},
		]),
	);
}

function buildTaskDiagnostics(): AdminTaskDiagnostics {
	return {
		business_outcome: {
			code: "partial",
			label: "Partial",
			message: "2 repos need retry after upstream rate limit.",
		},
		sync_subscriptions: {
			trigger: "manual",
			schedule_key: null,
			skipped: false,
			skip_reason: null,
			log_available: true,
			log_download_path: "/api/admin/jobs/realtime/task-sync-subscriptions/log",
			star: {
				total_users: 28,
				succeeded_users: 27,
				failed_users: 1,
				total_repos: 92,
			},
			release: {
				total_repos: 92,
				succeeded_repos: 88,
				failed_repos: 2,
				candidate_failures: 2,
				fetched_count: 1140,
				inserted_count: 140,
				updated_count: 88,
				unchanged_count: 912,
				pages_fetched: 92,
			},
			social: {
				total_users: 28,
				succeeded_users: 28,
				failed_users: 0,
				repo_stars: 32,
				followers: 4,
				events: 128,
			},
			notifications: {
				total_users: 28,
				succeeded_users: 28,
				failed_users: 0,
				notifications: 41,
			},
			releases_written: 1140,
			critical_events: 2,
			recent_events: [
				{
					id: "evt-1",
					stage: "release",
					event_type: "candidate.failed",
					severity: "warn",
					recoverable: true,
					attempt: 2,
					user_id: USER_ID,
					repo_id: 1002,
					repo_full_name: DOCS_REPO_FULL_NAME,
					message: "upstream GitHub API secondary rate limit",
					created_at: NOW,
				},
			],
		},
	};
}

function buildAdminJobs(): DemoJobsModel {
	const realtimeTasks: AdminRealtimeTaskItem[] = [
		{
			id: "task-sync-subscriptions",
			task_type: "sync.subscriptions",
			status: "running",
			source: "manual",
			requested_by: ADMIN_USER_ID,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-07-08T09:10:00+08:00",
			started_at: "2026-07-08T09:10:02+08:00",
			finished_at: null,
			updated_at: NOW,
			diagnostics: buildTaskDiagnostics(),
		},
		{
			id: "task-sync-releases",
			task_type: "sync.releases",
			status: "succeeded",
			source: "manual",
			requested_by: ADMIN_USER_ID,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-07-08T08:42:00+08:00",
			started_at: "2026-07-08T08:42:02+08:00",
			finished_at: "2026-07-08T08:43:12+08:00",
			updated_at: "2026-07-08T08:43:12+08:00",
		},
	];

	const scheduledRuns: AdminRealtimeTaskItem[] = [
		{
			id: "task-brief-slot-08",
			task_type: "brief.daily_slot",
			status: "running",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-07-08T08:00:00+08:00",
			started_at: "2026-07-08T08:00:02+08:00",
			finished_at: null,
			updated_at: NOW,
		},
	];

	const subscriptionRuns = realtimeTasks.filter(
		(item) => item.task_type === "sync.subscriptions",
	);

	const llmCalls: AdminLlmCallItem[] = [
		{
			id: "llm-call-1",
			status: "running",
			source: "job.api.translate_release",
			model: "gpt-5-mini",
			requested_by: ADMIN_USER_ID,
			parent_task_id: "task-sync-subscriptions",
			parent_task_type: "sync.subscriptions",
			max_tokens: 900,
			attempt_count: 1,
			scheduler_wait_ms: 220,
			first_token_wait_ms: 840,
			duration_ms: null,
			input_tokens: 1220,
			output_tokens: null,
			cached_input_tokens: 0,
			total_tokens: null,
			created_at: "2026-07-08T09:10:05+08:00",
			started_at: "2026-07-08T09:10:06+08:00",
			finished_at: null,
			updated_at: NOW,
		},
	];

	const llmCallDetails: Record<string, AdminLlmCallDetailResponse> = {
		"llm-call-1": {
			...llmCalls[0],
			input_messages_json: JSON.stringify([
				{
					role: "system",
					content:
						"You translate GitHub release notes into concise Chinese summaries.",
				},
			]),
			output_messages_json: null,
			prompt_text: "Translate release notes for /demo/ Web Demo contract",
			response_text: null,
			error_text: null,
		},
	};

	const taskDetails: Record<string, AdminRealtimeTaskDetailResponse> = {
		"task-sync-subscriptions": {
			task: {
				...realtimeTasks[0],
				payload_json: JSON.stringify({
					trigger: "manual",
					scope: "all-followed-releases",
				}),
				result_json: null,
			},
			events: [
				{
					id: "task-evt-1",
					event_type: "task.progress",
					payload_json: JSON.stringify({
						stage: "release_summary",
						succeeded: 88,
						total: 92,
					}),
					created_at: NOW,
				},
			],
			event_meta: {
				returned: 1,
				total: 1,
				limit: 50,
				truncated: false,
			},
			diagnostics: buildTaskDiagnostics(),
		},
	};

	return {
		overview: {
			queued: 1,
			running: 2,
			failed_24h: 1,
			succeeded_24h: 19,
			enabled_scheduled_slots: 12,
			total_scheduled_slots: 24,
		},
		realtimeTasks,
		scheduledRuns,
		subscriptionRuns,
		syncRuntimeConfig: {
			sync_auto_fetch_interval_minutes: 60,
			retry_recent_failures_interval_minutes: 30,
			repo_release_worker_concurrency: 6,
			repo_refresh_system_budget_per_window: 800,
			recent_sync_tasks: [
				{
					id: "task-sync-releases",
					status: "succeeded",
					source: "manual",
					skipped: false,
					duration_ms: 70123,
					created_at: "2026-07-08T08:42:00+08:00",
					started_at: "2026-07-08T08:42:02+08:00",
					finished_at: "2026-07-08T08:43:12+08:00",
				},
			],
		},
		llmStatus: {
			scheduler_enabled: true,
			llm_models: ["gpt-5-mini", "gpt-4.1-mini"],
			selected_model_for_new_calls: "gpt-5-mini",
			max_concurrency: 4,
			ai_model_context_limit: 120000,
			effective_model_input_limit: 120000,
			effective_model_input_limit_source: "runtime-config",
			model_statuses: [
				{
					model: "gpt-5-mini",
					priority: 1,
					status: "healthy",
					consecutive_final_failures: 0,
					cooldown_until: null,
					effective_input_limit: 120000,
					effective_input_limit_source: "runtime-config",
				},
			],
			available_slots: 3,
			waiting_calls: 1,
			in_flight_calls: 1,
			calls_24h: 182,
			failed_24h: 3,
			avg_wait_ms_24h: 540,
			avg_duration_ms_24h: 2800,
			last_success_at: "2026-07-08T09:08:00+08:00",
			last_failure_at: "2026-07-08T06:15:00+08:00",
		},
		llmCalls,
		taskDetails,
		llmCallDetails,
	};
}

function buildTaskStreams(): Record<
	string,
	{ delayMs: number; type: string; data?: unknown }[]
> {
	return {
		"task-demo-sync-all": [
			{
				delayMs: 120,
				type: "task.running",
				data: { task_id: "task-demo-sync-all" },
			},
			{
				delayMs: 420,
				type: "task.progress",
				data: { stage: "star_refreshed", succeeded: 14, total: 14 },
			},
			{
				delayMs: 980,
				type: "task.progress",
				data: { stage: "release_summary", succeeded: 24, total: 24 },
			},
			{
				delayMs: 1380,
				type: "task.progress",
				data: { stage: "social_summary", succeeded: 8, total: 8 },
			},
			{
				delayMs: 1820,
				type: "task.progress",
				data: { stage: "notifications_summary", succeeded: 4, total: 4 },
			},
			{
				delayMs: 2400,
				type: "task.completed",
				data: { status: "succeeded" },
			},
		],
		"task-demo-sync-inbox": [
			{
				delayMs: 120,
				type: "task.running",
				data: { task_id: "task-demo-sync-inbox" },
			},
			{
				delayMs: 720,
				type: "task.completed",
				data: { status: "succeeded" },
			},
		],
	};
}

function buildAdminJobsStream(): {
	delayMs: number;
	type: string;
	data?: unknown;
}[] {
	return [
		{
			delayMs: 1800,
			type: "job",
			data: {
				event_id: "job-evt-1",
				task_id: "task-sync-subscriptions",
				task_type: "sync.subscriptions",
				status: "running",
				event_type: "task.progress",
				created_at: NOW,
			},
		},
	];
}

export function buildDemoModel(input: {
	personaId: DemoPersonaId;
	includeOwnReleases: boolean;
	publicationState: DemoPublicationState;
}): DemoModel {
	const me = buildMe(input.personaId, input.includeOwnReleases);
	const profile = buildProfile(me, input.includeOwnReleases);
	const adminUsers = buildAdminUsers();
	const adminUserProfiles = buildAdminUserProfiles(adminUsers);

	return {
		me,
		profile,
		githubConnections: buildGitHubConnections(),
		linuxdo: buildLinuxDo(),
		passkeys: buildPasskeys(),
		apiKeys: buildApiKeys(),
		reactionToken: buildReactionToken(),
		followingRepos: buildFollowingRepos(),
		feed: buildFeed(input.includeOwnReleases),
		briefs: buildBriefs(),
		notifications: buildNotifications(),
		publicReleaseDetail: buildPublicReleaseDetail(),
		publicationStatus: buildPublicationStatus(input.publicationState),
		adminUsers,
		adminUserProfiles,
		adminJobs: buildAdminJobs(),
		taskStreams: buildTaskStreams(),
		adminJobsStream: buildAdminJobsStream(),
	};
}
