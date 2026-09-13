import type { AdminUserItem } from "@/admin/UserManagement";
import type {
	AdminJobsOverviewResponse,
	AdminLlmCallDetailResponse,
	AdminLlmCallItem,
	AdminLlmActivityResponse,
	AdminLlmSchedulerStatusResponse,
	AdminRealtimeTaskDetailResponse,
	AdminRealtimeTaskItem,
	AdminSyncRuntimeConfigResponse,
	AdminTranslationAttemptEvent,
	AdminTranslationBatchDetailResponse,
	AdminTranslationBatchListItem,
	AdminTranslationRequestDetailResponse,
	AdminTranslationRequestListItem,
	AdminTranslationStatusResponse,
	AdminUserProfileResponse,
	AdminWebhookPushRuntimeConfigResponse,
	AdminRepoGovernanceOverviewResponse,
	AdminRepoGovernanceListResponse,
	AnnouncementDetailResponse,
	AuthBindContextResponse,
	FollowingReposResponse,
	PersonalReposResponse,
	GitHubConnectionResponse,
	MeLinuxDoResponse,
	MeProfileResponse,
	MeResponse,
	PublicReleaseListResponse,
	ReactionTokenStatusResponse,
	ReleaseDetailResponse,
	RepoPublicReleasePublicationStatusResponse,
} from "@/api";
import type { FeedResponse } from "@/feed/types";
import type { NotificationItem } from "@/sidebar/InboxQuickList";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";
import type { PasskeySummary, ApiKeySummary } from "@/api";

export type DemoSceneId =
	| "landing-welcome"
	| "app-boot"
	| "app-shell"
	| "dashboard-repo-publish"
	| "settings-my-releases"
	| "public-release-ready"
	| "public-release-highlight-discrete"
	| "public-release-highlight-range"
	| "admin-panel-users"
	| "admin-dashboard-overview"
	| "admin-repos-overview"
	| "admin-jobs-running"
	| "admin-translation-audit"
	| "bind-github-pending"
	| "announcement-detail"
	| "not-found"
	| "paused-account-resume";

export type DemoPersonaId = "guest" | "member" | "admin";

export type DemoNetworkMode = "normal" | "slow" | "faulty" | "readable-loading";

export type DemoPublicationState = "published" | "unpublished";

export type DemoLandingCase =
	| "default"
	| "custom"
	| "github-redirect"
	| "linuxdo-redirect"
	| "passkey-authenticate"
	| "passkey-register"
	| "passkey-unsupported"
	| "auth-network-unavailable";

export type DemoLandingAuthAction =
	| "idle"
	| "github"
	| "linuxdo"
	| "passkey-authenticate"
	| "passkey-register";

export type DemoLandingPasskeySupport = "supported" | "unsupported";

export type DemoLandingBootState = "ready" | "network-unavailable";

export type DemoAppShellState =
	| "steady"
	| "update"
	| "install"
	| "update-install"
	| "unknown";

export type DemoMutationRecord = {
	id: string;
	label: string;
	detail: string;
	at: string;
};

export type DemoEventFrame = {
	delayMs: number;
	type: string;
	data?: unknown;
};

export type DemoJobsModel = {
	overview: AdminJobsOverviewResponse;
	realtimeTasks: AdminRealtimeTaskItem[];
	scheduledRuns: AdminRealtimeTaskItem[];
	subscriptionRuns: AdminRealtimeTaskItem[];
	syncRuntimeConfig: AdminSyncRuntimeConfigResponse;
	webhookPushRuntimeConfig: AdminWebhookPushRuntimeConfigResponse;
	llmStatus: AdminLlmSchedulerStatusResponse;
	llmActivity: AdminLlmActivityResponse;
	llmCalls: AdminLlmCallItem[];
	taskDetails: Record<string, AdminRealtimeTaskDetailResponse>;
	llmCallDetails: Record<string, AdminLlmCallDetailResponse>;
	translationStatus: AdminTranslationStatusResponse;
	translationRequests: AdminTranslationRequestListItem[];
	translationBatches: AdminTranslationBatchListItem[];
	translationAttemptEvents: AdminTranslationAttemptEvent[];
	translationRequestDetails: Record<
		string,
		AdminTranslationRequestDetailResponse
	>;
	translationBatchDetails: Record<string, AdminTranslationBatchDetailResponse>;
};

export type DemoModel = {
	me: MeResponse | null;
	profile: MeProfileResponse;
	githubConnections: GitHubConnectionResponse[];
	linuxdo: MeLinuxDoResponse;
	passkeys: PasskeySummary[];
	apiKeys: ApiKeySummary[];
	reactionToken: ReactionTokenStatusResponse;
	followingRepos: FollowingReposResponse;
	personalRepos: PersonalReposResponse;
	feed: FeedResponse;
	briefs: BriefItem[];
	notifications: NotificationItem[];
	publicReleaseDetail: ReleaseDetailResponse;
	publicReleaseList: PublicReleaseListResponse;
	publicationStatus: RepoPublicReleasePublicationStatusResponse;
	adminRepoGovernance: {
		overview: AdminRepoGovernanceOverviewResponse;
		list: AdminRepoGovernanceListResponse;
	};
	bindContext: AuthBindContextResponse;
	announcementDetail: AnnouncementDetailResponse;
	adminUsers: AdminUserItem[];
	adminUserProfiles: Record<string, AdminUserProfileResponse>;
	adminJobs: DemoJobsModel;
	taskStreams: Record<string, DemoEventFrame[]>;
	adminJobsStream: DemoEventFrame[];
};

export type DemoScene = {
	id: DemoSceneId;
	title: string;
	description: string;
	path: string;
	defaultPersona: DemoPersonaId;
	personas: DemoPersonaId[];
};

export type DemoShareState = {
	sceneId: DemoSceneId;
	personaId: DemoPersonaId;
	networkMode: DemoNetworkMode;
	includeOwnReleases: boolean;
	publicationState: DemoPublicationState;
	landingCase: DemoLandingCase;
	landingAuthAction: DemoLandingAuthAction;
	landingPasskeySupport: DemoLandingPasskeySupport;
	landingBootState: DemoLandingBootState;
	appShellState: DemoAppShellState;
	controlsHidden: boolean;
};

export type DemoShareStatePatch = Partial<DemoShareState>;

export type DemoPanelLayout = {
	edge: "left" | "right";
	x: number;
	y: number;
	collapsed: boolean;
};

export type DemoSnapshot = {
	active: boolean;
	demoBuild: boolean;
	basepath: string;
	revision: number;
	shareState: DemoShareState;
	model: DemoModel | null;
	mutations: DemoMutationRecord[];
	panelLayout: DemoPanelLayout;
	lastSyncedHref: string | null;
};
