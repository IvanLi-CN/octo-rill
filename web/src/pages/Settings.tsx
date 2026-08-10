import {
	ArrowLeft,
	CalendarClock,
	Copy,
	Fingerprint,
	Github,
	KeyRound,
	LoaderCircle,
	Link2,
	Package,
	Plus,
	ShieldAlert,
	SearchCheck,
	Trash2,
	Unlink2,
	Webhook,
} from "lucide-react";
import {
	type FormEvent,
	type ReactNode,
	useCallback,
	useEffect,
	useMemo,
	useState,
} from "react";

import {
	type ApiKeySummary,
	type CreateApiKeyResponse,
	type DailyBriefProfilePatchRequest,
	type GitHubConnectionResponse,
	type LinuxDoConnectionResponse,
	type MeResponse,
	type PasskeySummary,
	type MeProfileResponse,
	type WebhookPushSettingsResponse,
	apiCreateMeApiKey,
	apiDeleteMeApiKey,
	apiDeleteMePasskey,
	apiDeleteMeGitHubConnection,
	apiDeleteMeLinuxDo,
	apiGetMeApiKeys,
	apiGetMeGitHubConnections,
	apiGetMeLinuxDo,
	apiGetMePasskeys,
	apiGetMeProfile,
	apiGetMeWebhookPush,
	apiPostPasskeyRegisterOptions,
	apiPostPasskeyRegisterVerify,
	apiPatchMeProfile,
	apiPatchMeWebhookPush,
	apiRegisterMeWebhookPush,
	apiCheckMeWebhookPush,
	apiDeleteMeWebhookPushHooks,
} from "@/api";
import {
	browserSupportsPasskeys,
	createPasskeyCredential,
	normalizePasskeyErrorMessage,
} from "@/auth/passkeys";
import {
	DailyBriefProfileForm,
	readHourAlignedBrowserTimeZone,
} from "@/briefs/DailyBriefProfileForm";
import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
	AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { resolveDemoSafeAuthHref } from "@/demo/auth";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import {
	buildSettingsHref,
	buildSettingsSearch,
	type SettingsSection,
} from "@/settings/routeState";
import { GitHubPatGuideCard } from "@/settings/GitHubPatGuideCard";
import { GitHubPatInput } from "@/settings/GitHubPatInput";
import { useReactionTokenEditor } from "@/settings/reactionTokenEditor";

const SECTION_META: Record<
	SettingsSection,
	{
		label: string;
		description: string;
	}
> = {
	linuxdo: {
		label: "LinuxDO 绑定",
		description:
			"绑定 LinuxDO Connect 账号，只保存本地快照，不存 LinuxDO PAT。",
	},
	passkeys: {
		label: "Passkeys",
		description:
			"给当前账号添加可直接登录的 Passkey；GitHub / LinuxDO 仍保留为恢复与补绑路径。",
	},
	"github-accounts": {
		label: "GitHub 账号",
		description:
			"一个 OctoRill 账号可绑定多个 GitHub 账号；全部绑定都会参与登录、同步与 PAT 校验。",
	},
	"my-releases": {
		label: "我的发布",
		description:
			"把你自己 owner 的仓库发布也纳入 release 阅读面，但不会写进真实加星列表。",
	},
	"github-pat": {
		label: "GitHub PAT",
		description:
			"配置 release feedback 所需的 GitHub PAT，保留 800ms 防抖校验。",
	},
	"api-keys": {
		label: "API Key",
		description: "创建用于调用用户态业务接口的 Bearer token。",
	},
	"daily-brief": {
		label: "日报设置",
		description: "调整日报生成边界，继续沿用现有 /api/me/profile 契约。",
	},
};

const PASSKEY_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	registered: {
		tone: "success",
		title: "Passkey 已添加",
		description: "这把 Passkey 现在可以直接用于登录当前 OctoRill 账号。",
	},
	deleted: {
		tone: "success",
		title: "Passkey 已移除",
		description: "已删除对应设备的 Passkey；GitHub / LinuxDO 登录不会受影响。",
	},
	passkey_retry_required: {
		tone: "error",
		title: "需要重新添加 Passkey",
		description:
			"这次 GitHub 绑定没有自动挂接之前创建的 Passkey，请在当前账号的设置页重新添加。",
	},
	passkey_already_bound: {
		tone: "error",
		title: "Passkey 已被占用",
		description: "这把 Passkey 已经绑定到其他 OctoRill 账号，不能重复添加。",
	},
	expired: {
		tone: "error",
		title: "Passkey 状态已过期",
		description: "之前暂存的 Passkey 已经过期，请重新添加一次。",
	},
};

const GITHUB_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	connected: {
		tone: "success",
		title: "GitHub 账号已绑定",
		description: "新的 GitHub 账号已加入当前 OctoRill 账号，可以参与聚合同步。",
	},
	already_bound: {
		tone: "error",
		title: "GitHub 账号已被占用",
		description: "这个 GitHub 账号已经绑定到其他 OctoRill 账号，不能重复绑定。",
	},
};

const LINUXDO_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	connected: {
		tone: "success",
		title: "LinuxDO 已绑定",
		description: "绑定快照已同步到当前账号，现在可以在这里查看或解绑。",
	},
	already_bound: {
		tone: "error",
		title: "LinuxDO 账号已被占用",
		description:
			"这个 LinuxDO 账号已经绑定到其他 OctoRill 用户，请换账号后重试。",
	},
	not_configured: {
		tone: "error",
		title: "LinuxDO Connect 尚未配置",
		description: "服务端缺少 LinuxDO OAuth 环境变量，当前环境无法发起绑定。",
	},
	state_mismatch: {
		tone: "error",
		title: "绑定状态校验失败",
		description: "OAuth state 不匹配，请重新发起一次 LinuxDO Connect 绑定。",
	},
	exchange_failed: {
		tone: "error",
		title: "LinuxDO 授权交换失败",
		description: "服务端没能完成 code → token 交换，请稍后重试。",
	},
	fetch_user_failed: {
		tone: "error",
		title: "LinuxDO 用户信息获取失败",
		description: "授权成功后没能读取 LinuxDO 用户资料，请稍后再试。",
	},
	save_failed: {
		tone: "error",
		title: "LinuxDO 绑定保存失败",
		description: "本地绑定快照写入失败，请稍后重试。",
	},
};

function formatDateTime(value: string | null | undefined) {
	if (!value) return "—";
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function avatarFallbackText(name: string | null | undefined, login: string) {
	const source = (name ?? login).trim();
	const normalized = source
		.split(/[\s_-]+/)
		.filter(Boolean)
		.slice(0, 2)
		.map((part) => part[0]?.toUpperCase() ?? "")
		.join("");
	return normalized || login.slice(0, 2).toUpperCase();
}

function statusToneClassName(tone: "success" | "error" | "idle" | "muted") {
	switch (tone) {
		case "success":
			return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
		case "error":
			return "border-destructive/30 bg-destructive/8 text-destructive";
		case "muted":
			return "border-border bg-muted/30 text-muted-foreground";
		default:
			return "border-border bg-background text-foreground";
	}
}

function DetailItem(props: {
	label: string;
	value: ReactNode;
	hint?: ReactNode;
	mono?: boolean;
}) {
	const { label, value, hint, mono = false } = props;
	return (
		<div className="space-y-1.5 rounded-xl border border-border/70 bg-background/80 p-3">
			<p className="text-muted-foreground text-[11px] font-medium tracking-[0.16em] uppercase">
				{label}
			</p>
			<div
				className={cn(
					"text-sm leading-6 text-foreground",
					mono && "font-mono text-[13px]",
				)}
			>
				{value}
			</div>
			{hint ? (
				<div className="text-muted-foreground text-xs leading-5">{hint}</div>
			) : null}
		</div>
	);
}

export function SettingsPage(props: {
	me: MeResponse;
	section: SettingsSection;
	githubStatus?: string | null;
	linuxdoStatus?: string | null;
	passkeyStatus?: string | null;
	passkeySupportOverride?: boolean | null;
	onSectionChange: (section: SettingsSection) => void;
	onProfileSaved?: () => Promise<void> | void;
}) {
	const {
		me,
		section,
		githubStatus = null,
		linuxdoStatus = null,
		passkeyStatus = null,
		passkeySupportOverride = null,
		onSectionChange,
		onProfileSaved,
	} = props;
	const [githubConnectionsLoading, setGitHubConnectionsLoading] =
		useState(true);
	const [githubConnectionsBusyId, setGitHubConnectionsBusyId] = useState<
		string | null
	>(null);
	const [githubConnectionsError, setGitHubConnectionsError] = useState<
		string | null
	>(null);
	const [githubConnections, setGitHubConnections] = useState<
		GitHubConnectionResponse[]
	>([]);
	const [passkeysLoading, setPasskeysLoading] = useState(true);
	const [passkeysRegistering, setPasskeysRegistering] = useState(false);
	const [passkeysBusyId, setPasskeysBusyId] = useState<string | null>(null);
	const [passkeysError, setPasskeysError] = useState<string | null>(null);
	const [passkeysSupported, setPasskeysSupported] = useState(false);
	const [passkeys, setPasskeys] = useState<PasskeySummary[]>([]);
	const [passkeyFlashStatus, setPasskeyFlashStatus] = useState<string | null>(
		passkeyStatus,
	);
	const [apiKeysLoading, setApiKeysLoading] = useState(true);
	const [apiKeysCreating, setApiKeysCreating] = useState(false);
	const [apiKeysBusyId, setApiKeysBusyId] = useState<string | null>(null);
	const [apiKeysError, setApiKeysError] = useState<string | null>(null);
	const [apiKeys, setApiKeys] = useState<ApiKeySummary[]>([]);
	const [apiKeyName, setApiKeyName] = useState("");
	const [createdApiKey, setCreatedApiKey] =
		useState<CreateApiKeyResponse | null>(null);
	const [copiedApiKeyId, setCopiedApiKeyId] = useState<string | null>(null);
	const [pendingApiKeyRevokeId, setPendingApiKeyRevokeId] = useState<
		string | null
	>(null);
	const [linuxdoLoading, setLinuxdoLoading] = useState(true);
	const [linuxdoBusy, setLinuxdoBusy] = useState(false);
	const [linuxdoError, setLinuxdoError] = useState<string | null>(null);
	const [linuxdoAvailable, setLinuxdoAvailable] = useState(false);
	const [linuxdoConnection, setLinuxdoConnection] =
		useState<LinuxDoConnectionResponse | null>(null);
	const {
		reactionTokenLoading,
		reactionTokenConfigured,
		reactionTokenMasked,
		reactionTokenOwner,
		patInput,
		setPatInput,
		patCheckState,
		patCheckMessage,
		patCheckedAt,
		patSaving,
		canSavePat,
		savePat,
	} = useReactionTokenEditor();

	const [briefProfileLoading, setBriefProfileLoading] = useState(true);
	const [briefProfileSaving, setBriefProfileSaving] = useState(false);
	const [ownReleaseSaving, setOwnReleaseSaving] = useState(false);
	const [, setBriefProfile] = useState<MeProfileResponse | null>(null);
	const [briefProfileError, setBriefProfileError] = useState<string | null>(
		null,
	);
	const [briefProfileDraft, setBriefProfileDraft] =
		useState<DailyBriefProfilePatchRequest>({
			daily_brief_time_zone:
				me.dashboard.daily_boundary_time_zone ??
				readHourAlignedBrowserTimeZone() ??
				"Asia/Shanghai",
		});
	const [includeOwnReleases, setIncludeOwnReleases] = useState(false);
	const [webhookPush, setWebhookPush] =
		useState<WebhookPushSettingsResponse | null>(null);
	const [webhookPushLoading, setWebhookPushLoading] = useState(true);
	const [webhookPushBusy, setWebhookPushBusy] = useState<string | null>(null);
	const [webhookPushError, setWebhookPushError] = useState<string | null>(null);
	const [webhookPushNotice, setWebhookPushNotice] = useState<string | null>(
		null,
	);
	const [webhookPushConfirmOpen, setWebhookPushConfirmOpen] = useState(false);

	const activeGitHubStatusMeta = githubStatus
		? (GITHUB_STATUS_META[githubStatus] ?? null)
		: null;
	const activePasskeyStatusMeta = passkeyFlashStatus
		? (PASSKEY_STATUS_META[passkeyFlashStatus] ?? null)
		: null;
	const activeStatusMeta = linuxdoStatus
		? (LINUXDO_STATUS_META[linuxdoStatus] ?? null)
		: null;

	const loadGitHubConnections = useCallback(async () => {
		setGitHubConnectionsLoading(true);
		setGitHubConnectionsError(null);
		try {
			const res = await apiGetMeGitHubConnections();
			setGitHubConnections(res.items);
		} catch (err) {
			setGitHubConnectionsError(
				err instanceof Error ? err.message : String(err),
			);
		} finally {
			setGitHubConnectionsLoading(false);
		}
	}, []);

	const loadLinuxDo = useCallback(async () => {
		setLinuxdoLoading(true);
		setLinuxdoError(null);
		try {
			const res = await apiGetMeLinuxDo();
			setLinuxdoAvailable(res.available);
			setLinuxdoConnection(res.connection);
		} catch (err) {
			setLinuxdoError(err instanceof Error ? err.message : String(err));
		} finally {
			setLinuxdoLoading(false);
		}
	}, []);

	const loadPasskeys = useCallback(async () => {
		setPasskeysLoading(true);
		setPasskeysError(null);
		try {
			const res = await apiGetMePasskeys();
			setPasskeys(res.items);
		} catch (err) {
			setPasskeysError(err instanceof Error ? err.message : String(err));
		} finally {
			setPasskeysLoading(false);
		}
	}, []);

	const loadApiKeys = useCallback(async () => {
		setApiKeysLoading(true);
		setApiKeysError(null);
		try {
			const res = await apiGetMeApiKeys();
			setApiKeys(res.items);
		} catch (err) {
			setApiKeysError(err instanceof Error ? err.message : String(err));
		} finally {
			setApiKeysLoading(false);
		}
	}, []);

	const loadBriefProfile = useCallback(async () => {
		setBriefProfileLoading(true);
		setBriefProfileError(null);
		try {
			const profile = await apiGetMeProfile();
			setBriefProfile(profile);
			setBriefProfileDraft({
				daily_brief_time_zone: profile.daily_brief_time_zone,
			});
			setIncludeOwnReleases(profile.include_own_releases);
		} catch (err) {
			setBriefProfileError(err instanceof Error ? err.message : String(err));
		} finally {
			setBriefProfileLoading(false);
		}
	}, []);

	const loadWebhookPush = useCallback(async () => {
		setWebhookPushLoading(true);
		setWebhookPushError(null);
		try {
			setWebhookPush(await apiGetMeWebhookPush());
		} catch (err) {
			setWebhookPushError(err instanceof Error ? err.message : String(err));
		} finally {
			setWebhookPushLoading(false);
		}
	}, []);

	useEffect(() => {
		setPasskeyFlashStatus(passkeyStatus);
	}, [passkeyStatus]);

	useEffect(() => {
		setPasskeysSupported(passkeySupportOverride ?? browserSupportsPasskeys());
	}, [passkeySupportOverride]);

	useEffect(() => {
		if (section !== "api-keys") {
			setCreatedApiKey(null);
			setCopiedApiKeyId(null);
			setPendingApiKeyRevokeId(null);
		}
	}, [section]);

	useEffect(() => {
		const loaders = [
			loadGitHubConnections(),
			loadPasskeys(),
			loadLinuxDo(),
			loadBriefProfile(),
		];
		if (section === "my-releases") {
			loaders.push(loadWebhookPush());
		}
		if (section === "api-keys") {
			loaders.push(loadApiKeys());
		}
		void Promise.all(loaders);
	}, [
		loadApiKeys,
		loadBriefProfile,
		loadGitHubConnections,
		loadLinuxDo,
		loadPasskeys,
		loadWebhookPush,
		section,
	]);

	const onConnectGitHub = useCallback(() => {
		window.location.assign(
			resolveDemoSafeAuthHref("/auth/github/connect", "connect"),
		);
	}, []);

	const onDeleteGitHub = useCallback(
		(connectionId: string) => {
			setGitHubConnectionsBusyId(connectionId);
			setGitHubConnectionsError(null);
			void apiDeleteMeGitHubConnection(connectionId)
				.then(async (res) => {
					setGitHubConnections(res.items);
					await onProfileSaved?.();
				})
				.catch((err) => {
					setGitHubConnectionsError(
						err instanceof Error ? err.message : String(err),
					);
				})
				.finally(() => {
					setGitHubConnectionsBusyId(null);
				});
		},
		[onProfileSaved],
	);

	const onConnectLinuxDo = useCallback(() => {
		window.location.assign(
			resolveDemoSafeAuthHref("/auth/linuxdo/login", "connect"),
		);
	}, []);

	const onRegisterPasskey = useCallback(() => {
		if (!passkeysSupported) {
			setPasskeysError(
				"当前浏览器不支持 Passkey，请改用 GitHub / LinuxDO 登录。",
			);
			return;
		}

		setPasskeysRegistering(true);
		setPasskeysError(null);
		void apiPostPasskeyRegisterOptions()
			.then((options) => createPasskeyCredential(options))
			.then((credential) => apiPostPasskeyRegisterVerify(credential))
			.then(async (res) => {
				setPasskeyFlashStatus(
					res.status === "registered" ? "registered" : null,
				);
				await loadPasskeys();
			})
			.catch((err) => {
				setPasskeysError(normalizePasskeyErrorMessage(err));
			})
			.finally(() => {
				setPasskeysRegistering(false);
			});
	}, [loadPasskeys, passkeysSupported]);

	const onDeletePasskey = useCallback((passkeyId: string) => {
		setPasskeysBusyId(passkeyId);
		setPasskeysError(null);
		void apiDeleteMePasskey(passkeyId)
			.then((res) => {
				setPasskeys(res.items);
				setPasskeyFlashStatus("deleted");
			})
			.catch((err) => {
				setPasskeysError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setPasskeysBusyId(null);
			});
	}, []);

	const onCreateApiKey = useCallback(
		(event: FormEvent<HTMLFormElement>) => {
			event.preventDefault();
			setApiKeysCreating(true);
			setApiKeysError(null);
			setCopiedApiKeyId(null);
			void apiCreateMeApiKey({
				name: apiKeyName.trim() || undefined,
			})
				.then((res) => {
					setCreatedApiKey(res);
					setApiKeys((current) => [
						res.item,
						...current.filter((item) => item.id !== res.item.id),
					]);
					setApiKeyName("");
				})
				.catch((err) => {
					setApiKeysError(err instanceof Error ? err.message : String(err));
				})
				.finally(() => {
					setApiKeysCreating(false);
				});
		},
		[apiKeyName],
	);

	const onCopyApiKey = useCallback((apiKeyId: string, apiKey: string) => {
		void navigator.clipboard
			.writeText(apiKey)
			.then(() => setCopiedApiKeyId(apiKeyId))
			.catch((err) => {
				setApiKeysError(err instanceof Error ? err.message : String(err));
			});
	}, []);

	const onDeleteApiKey = useCallback((apiKeyId: string) => {
		setApiKeysBusyId(apiKeyId);
		setApiKeysError(null);
		void apiDeleteMeApiKey(apiKeyId)
			.then((res) => {
				setApiKeys(res.items);
				setCreatedApiKey((current) =>
					current?.item.id === apiKeyId ? null : current,
				);
				setPendingApiKeyRevokeId(null);
			})
			.catch((err) => {
				setApiKeysError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setApiKeysBusyId(null);
			});
	}, []);

	const onDisconnectLinuxDo = useCallback(() => {
		setLinuxdoBusy(true);
		setLinuxdoError(null);
		void apiDeleteMeLinuxDo()
			.then((res) => {
				setLinuxdoAvailable(res.available);
				setLinuxdoConnection(res.connection);
			})
			.catch((err) => {
				setLinuxdoError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setLinuxdoBusy(false);
			});
	}, []);

	const onSavePat = useCallback(() => {
		void savePat();
	}, [savePat]);

	const onSaveBriefProfile = useCallback(() => {
		setBriefProfileSaving(true);
		setBriefProfileError(null);
		void apiPatchMeProfile({
			...briefProfileDraft,
			include_own_releases: includeOwnReleases,
		})
			.then(async (profile) => {
				setBriefProfile(profile);
				setBriefProfileDraft({
					daily_brief_time_zone: profile.daily_brief_time_zone,
				});
				setIncludeOwnReleases(profile.include_own_releases);
				await onProfileSaved?.();
			})
			.catch((err) => {
				setBriefProfileError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setBriefProfileSaving(false);
			});
	}, [briefProfileDraft, includeOwnReleases, onProfileSaved]);

	const onSaveOwnReleases = useCallback(() => {
		setOwnReleaseSaving(true);
		setBriefProfileError(null);
		void apiPatchMeProfile({
			...briefProfileDraft,
			include_own_releases: includeOwnReleases,
		})
			.then(async (profile) => {
				setBriefProfile(profile);
				setBriefProfileDraft({
					daily_brief_time_zone: profile.daily_brief_time_zone,
				});
				setIncludeOwnReleases(profile.include_own_releases);
				await onProfileSaved?.();
			})
			.catch((err) => {
				setBriefProfileError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setOwnReleaseSaving(false);
			});
	}, [briefProfileDraft, includeOwnReleases, onProfileSaved]);

	const runWebhookPushAction = useCallback(
		async (action: "register" | "check" | "delete", repoId?: number) => {
			const busyKey = repoId === undefined ? action : `${action}:${repoId}`;
			setWebhookPushBusy(busyKey);
			setWebhookPushError(null);
			setWebhookPushNotice(null);
			try {
				const task =
					action === "register"
						? await apiRegisterMeWebhookPush(repoId)
						: action === "check"
							? await apiCheckMeWebhookPush(repoId)
							: await apiDeleteMeWebhookPushHooks();
				setWebhookPushNotice(
					task.reused
						? "相同操作已在队列中，无需重复提交。"
						: `${action === "register" ? "注册" : action === "check" ? "检查" : "删除"}任务已排队。`,
				);
				window.setTimeout(() => void loadWebhookPush(), 1200);
			} catch (err) {
				setWebhookPushError(err instanceof Error ? err.message : String(err));
			} finally {
				setWebhookPushBusy(null);
			}
		},
		[loadWebhookPush],
	);

	const onWebhookPushToggle = useCallback(
		(checked: boolean) => {
			setWebhookPushError(null);
			setWebhookPushNotice(null);
			if (!checked) {
				setWebhookPushBusy("toggle");
				void apiPatchMeWebhookPush(false)
					.then(() => loadWebhookPush())
					.catch((err) =>
						setWebhookPushError(
							err instanceof Error ? err.message : String(err),
						),
					)
					.finally(() => setWebhookPushBusy(null));
				return;
			}
			if (!includeOwnReleases) {
				setWebhookPushError("请先开启并保存“我的发布”，再启用 Webhook 推送。");
				return;
			}
			if (!webhookPush?.pat.configured || !webhookPush.pat.valid) {
				setWebhookPushError(
					"请先在 GitHub PAT 中保存属于当前绑定账号、具备 repo 或 public_repo 权限的 classic PAT。",
				);
				return;
			}
			if (!webhookPush.callback_ready) {
				setWebhookPushError(
					"当前实例未配置可从 GitHub 访问的 HTTPS 回调地址，请联系管理员配置 OCTORILL_PUBLIC_BASE_URL。",
				);
				return;
			}
			setWebhookPushConfirmOpen(true);
		},
		[includeOwnReleases, webhookPush],
	);

	const confirmWebhookPushEnable = useCallback(() => {
		setWebhookPushBusy("toggle");
		setWebhookPushError(null);
		void apiPatchMeWebhookPush(true)
			.then(async () => {
				await loadWebhookPush();
				setWebhookPushConfirmOpen(false);
				setWebhookPushNotice("Webhook 推送已开启，全量注册任务已排队。");
			})
			.catch((err) =>
				setWebhookPushError(err instanceof Error ? err.message : String(err)),
			)
			.finally(() => setWebhookPushBusy(null));
	}, [loadWebhookPush]);

	const patTone = useMemo(() => {
		if (patCheckState === "valid") return "success";
		if (patCheckState === "invalid" || patCheckState === "error")
			return "error";
		if (patCheckState === "checking") return "muted";
		return "idle";
	}, [patCheckState]);

	const briefSummary = useMemo(() => {
		if (briefProfileLoading) return "读取中";
		return `按 ${briefProfileDraft.daily_brief_time_zone} 解释昨天`;
	}, [briefProfileDraft.daily_brief_time_zone, briefProfileLoading]);
	const ownReleaseSummary = includeOwnReleases ? "已开启" : "已关闭";
	const profileBusy =
		briefProfileLoading || briefProfileSaving || ownReleaseSaving;

	const linuxdoStatusBadge = linuxdoLoading
		? { label: "读取中", variant: "outline" as const }
		: linuxdoConnection
			? { label: "已绑定", variant: "secondary" as const }
			: linuxdoAvailable
				? { label: "未绑定", variant: "outline" as const }
				: { label: "未启用", variant: "outline" as const };

	const patStatusBadge =
		patCheckState === "valid"
			? { label: "PAT 可用", variant: "secondary" as const }
			: patCheckState === "invalid"
				? { label: "PAT 无效", variant: "destructive" as const }
				: patCheckState === "error"
					? { label: "校验异常", variant: "destructive" as const }
					: patCheckState === "checking"
						? { label: "校验中", variant: "outline" as const }
						: reactionTokenConfigured
							? { label: "已配置", variant: "secondary" as const }
							: { label: "未配置", variant: "outline" as const };

	const githubStatusBadge = githubConnectionsLoading
		? { label: "读取中", variant: "outline" as const }
		: githubConnections.length > 0
			? {
					label:
						githubConnections.length === 1
							? "1 个 GitHub 账号"
							: `${githubConnections.length} 个 GitHub 账号`,
					variant: "secondary" as const,
				}
			: { label: "未绑定", variant: "outline" as const };

	const passkeyStatusBadge = passkeysLoading
		? { label: "读取中", variant: "outline" as const }
		: passkeys.length > 0
			? {
					label:
						passkeys.length === 1
							? "1 把 Passkey"
							: `${passkeys.length} 把 Passkey`,
					variant: "secondary" as const,
				}
			: { label: "未添加", variant: "outline" as const };

	const apiKeyStatusBadge = apiKeysLoading
		? { label: "读取中", variant: "outline" as const }
		: apiKeys.length > 0
			? {
					label: apiKeys.length === 1 ? "1 把 Key" : `${apiKeys.length} 把 Key`,
					variant: "secondary" as const,
				}
			: { label: "未创建", variant: "outline" as const };

	const sectionNavItems = [
		{
			id: "passkeys" as const,
			icon: <Fingerprint className="size-4" />,
		},
		{
			id: "github-accounts" as const,
			icon: <Github className="size-4" />,
		},
		{
			id: "daily-brief" as const,
			icon: <CalendarClock className="size-4" />,
		},
		{
			id: "my-releases" as const,
			icon: <Package className="size-4" />,
		},
		{
			id: "github-pat" as const,
			icon: <KeyRound className="size-4" />,
		},
		{
			id: "api-keys" as const,
			icon: <KeyRound className="size-4" />,
		},
		{
			id: "linuxdo" as const,
			icon: <Link2 className="size-4" />,
		},
	];

	return (
		<AppShell
			header={
				<div className="flex flex-wrap items-center justify-between gap-3">
					<div className="flex min-w-0 items-center gap-3">
						<div className="size-9 shrink-0 rounded-2xl border border-border/60 bg-card p-1.5 shadow-sm">
							<BrandLogo
								variant="mark"
								alt=""
								className="size-full"
								imgClassName="size-full"
							/>
						</div>
						<div className="min-w-0">
							<p className="text-muted-foreground text-xs font-medium tracking-[0.18em] uppercase">
								Settings
							</p>
							<h1 className="truncate text-xl font-semibold tracking-tight text-foreground">
								账号与偏好
							</h1>
						</div>
					</div>
					<div className="flex items-center gap-2">
						<ThemeToggle />
						<Button asChild variant="outline" size="sm">
							<InternalLink href="/" to="/">
								<ArrowLeft className="size-4" />
								返回工作台
							</InternalLink>
						</Button>
					</div>
				</div>
			}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="mx-auto w-full max-w-6xl space-y-4" data-settings-layout>
				{activeGitHubStatusMeta ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName(activeGitHubStatusMeta.tone),
						)}
					>
						<div className="flex items-start gap-2.5">
							{activeGitHubStatusMeta.tone === "error" ? (
								<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							) : (
								<Github className="mt-0.5 size-4 shrink-0" />
							)}
							<div className="space-y-0.5">
								<p className="font-medium">{activeGitHubStatusMeta.title}</p>
								<p className="text-xs leading-5">
									{activeGitHubStatusMeta.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				{activePasskeyStatusMeta ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName(activePasskeyStatusMeta.tone),
						)}
					>
						<div className="flex items-start gap-2.5">
							{activePasskeyStatusMeta.tone === "error" ? (
								<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							) : (
								<Fingerprint className="mt-0.5 size-4 shrink-0" />
							)}
							<div className="space-y-0.5">
								<p className="font-medium">{activePasskeyStatusMeta.title}</p>
								<p className="text-xs leading-5">
									{activePasskeyStatusMeta.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				{activeStatusMeta ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName(activeStatusMeta.tone),
						)}
					>
						<div className="flex items-start gap-2.5">
							{activeStatusMeta.tone === "error" ? (
								<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							) : (
								<Link2 className="mt-0.5 size-4 shrink-0" />
							)}
							<div className="space-y-0.5">
								<p className="font-medium">{activeStatusMeta.title}</p>
								<p className="text-xs leading-5">
									{activeStatusMeta.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				<nav data-settings-nav className="grid grid-cols-2 gap-2 md:hidden">
					{sectionNavItems.map((item) => {
						const active = section === item.id;
						return (
							<InternalLink
								key={item.id}
								href={buildSettingsHref(item.id)}
								to="/settings"
								search={buildSettingsSearch(item.id)}
								onClick={() => onSectionChange(item.id)}
								className={cn(
									"flex h-11 w-full items-center gap-2 rounded-xl border px-3 text-sm font-medium transition-colors",
									active
										? "border-foreground/90 bg-foreground text-background shadow-sm"
										: "border-border/70 bg-background/85 text-foreground",
								)}
							>
								{item.icon}
								<span className="truncate">{SECTION_META[item.id].label}</span>
							</InternalLink>
						);
					})}
				</nav>

				<nav
					data-settings-nav
					className="hidden flex-wrap rounded-2xl border border-border/70 bg-card/95 p-2 shadow-sm md:flex"
				>
					{sectionNavItems.map((item) => (
						<Button
							key={item.id}
							asChild
							variant={section === item.id ? "default" : "ghost"}
							size="sm"
							className="h-9 w-auto rounded-xl px-3"
						>
							<InternalLink
								href={buildSettingsHref(item.id)}
								to="/settings"
								search={buildSettingsSearch(item.id)}
								onClick={() => onSectionChange(item.id)}
							>
								{item.icon}
								{SECTION_META[item.id].label}
							</InternalLink>
						</Button>
					))}
				</nav>

				<div className="min-w-0 max-md:border-t max-md:border-border/60 max-md:pt-4">
					{section === "passkeys" ? (
						<section id="settings-passkeys" data-settings-section="passkeys">
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META.passkeys.label}
											</CardTitle>
											<Badge variant={passkeyStatusBadge.variant}>
												{passkeyStatusBadge.label}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={!passkeysSupported || passkeysRegistering}
											onClick={onRegisterPasskey}
											data-settings-passkey-register
										>
											{passkeysRegistering ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<Fingerprint className="size-4" />
											)}
											添加 Passkey
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									{passkeysError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{passkeysError}
										</div>
									) : null}

									<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-muted-foreground">
										Passkey 只负责直接登录当前账号；GitHub / LinuxDO
										仍保留为恢复路径。若当前浏览器不支持 Passkey，请继续用
										GitHub 或 LinuxDO 登录。
									</div>

									{!passkeysSupported ? (
										<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-foreground">
											当前浏览器不支持 Passkey 注册；你仍然可以继续使用 GitHub /
											LinuxDO 登录。
										</div>
									) : null}

									{passkeysLoading ? (
										<div className="text-muted-foreground flex items-center gap-2 text-sm">
											<LoaderCircle className="size-4 animate-spin" />
											正在读取 Passkey 列表…
										</div>
									) : passkeys.length === 0 ? (
										<div className="rounded-2xl border border-border/70 bg-background/80 px-4 py-4 text-sm leading-6 text-foreground">
											<p className="font-medium">
												还没有可直接登录的 Passkey。
											</p>
											<p className="text-muted-foreground mt-1">
												添加后，你可以跳过 GitHub OAuth
												重定向，直接用浏览器或系统设备登录。
											</p>
										</div>
									) : (
										<div className="space-y-3">
											{passkeys.map((passkey) => {
												const isBusy = passkeysBusyId === passkey.id;
												return (
													<div
														key={passkey.id}
														className="space-y-3 rounded-2xl border border-border/70 bg-background/80 p-4 sm:p-5"
														data-passkey-item={passkey.id}
													>
														<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
															<div className="space-y-1.5">
																<p className="text-base font-semibold text-foreground">
																	{passkey.label}
																</p>
																<p className="text-muted-foreground text-sm leading-6">
																	注册后可直接登录当前 OctoRill 账号。
																</p>
															</div>
															<Button
																variant="outline"
																size="sm"
																disabled={isBusy}
																onClick={() => onDeletePasskey(passkey.id)}
															>
																{isBusy ? (
																	<LoaderCircle className="size-4 animate-spin" />
																) : (
																	<Unlink2 className="size-4" />
																)}
																移除
															</Button>
														</div>

														<div className="grid gap-3 sm:grid-cols-2">
															<DetailItem
																label="添加时间"
																value={formatDateTime(passkey.created_at)}
															/>
															<DetailItem
																label="最近使用"
																value={formatDateTime(passkey.last_used_at)}
																hint="如果是新添加但还没用来登录，这里会保持为空。"
															/>
														</div>
													</div>
												);
											})}
										</div>
									)}
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "github-accounts" ? (
						<section
							id="settings-github-accounts"
							data-settings-section="github-accounts"
						>
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["github-accounts"].label}
											</CardTitle>
											<Badge variant={githubStatusBadge.variant}>
												{githubStatusBadge.label}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={githubConnectionsLoading}
											onClick={onConnectGitHub}
										>
											<AuthProviderIcon provider="github" />
											绑定 GitHub
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									{githubConnectionsError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{githubConnectionsError}
										</div>
									) : null}

									<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-muted-foreground">
										全部已绑定 GitHub 账号都可以直接登录当前 OctoRill
										账号，也都会参与同步与 PAT 校验。
									</div>

									{githubConnectionsLoading ? (
										<div className="text-muted-foreground flex items-center gap-2 text-sm">
											<LoaderCircle className="size-4 animate-spin" />
											正在读取 GitHub 绑定列表…
										</div>
									) : (
										<div className="space-y-3">
											{githubConnections.map((connection) => {
												const isBusy =
													githubConnectionsBusyId === connection.id;
												return (
													<div
														key={connection.id}
														className="space-y-4 rounded-2xl border border-border/70 bg-background/80 p-4 sm:p-5"
													>
														<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
															<div className="flex min-w-0 items-center gap-3">
																{connection.avatar_url ? (
																	<img
																		src={connection.avatar_url}
																		alt={`${connection.login} avatar`}
																		className="size-12 shrink-0 rounded-full border border-border/70 object-cover"
																		referrerPolicy="no-referrer"
																		data-github-connection-avatar={
																			connection.login
																		}
																	/>
																) : (
																	<div
																		className="bg-muted text-muted-foreground flex size-12 shrink-0 items-center justify-center rounded-full border border-border/70 text-xs font-semibold"
																		data-github-connection-avatar-fallback={
																			connection.login
																		}
																	>
																		{avatarFallbackText(
																			connection.name,
																			connection.login,
																		)}
																	</div>
																)}
																<div className="min-w-0 space-y-1.5">
																	<div className="flex flex-wrap items-center gap-2">
																		<p className="truncate text-base font-semibold text-foreground">
																			{connection.name ?? connection.login}
																		</p>
																		<Badge
																			variant="outline"
																			className="rounded-full px-2.5 py-0.5 text-[11px] font-medium"
																		>
																			ID {connection.github_user_id}
																		</Badge>
																	</div>
																	<div className="text-muted-foreground flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
																		<span className="truncate">
																			@{connection.login}
																		</span>
																		{connection.email ? (
																			<>
																				<span className="hidden sm:inline">
																					·
																				</span>
																				<span className="truncate">
																					{connection.email}
																				</span>
																			</>
																		) : null}
																	</div>
																</div>
															</div>
															<div className="flex flex-wrap gap-2">
																{githubConnections.length <= 1 ? null : (
																	<Button
																		variant="outline"
																		size="sm"
																		disabled={isBusy}
																		onClick={() =>
																			onDeleteGitHub(connection.id)
																		}
																	>
																		{isBusy ? (
																			<LoaderCircle className="size-4 animate-spin" />
																		) : (
																			<Unlink2 className="size-4" />
																		)}
																		解绑
																	</Button>
																)}
															</div>
														</div>

														<div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_18rem]">
															<div className="space-y-2 rounded-xl border border-border/70 bg-muted/20 px-4 py-3">
																<p className="text-muted-foreground text-[11px] font-medium tracking-[0.16em] uppercase">
																	Scope
																</p>
																<p className="break-words text-sm leading-6 text-foreground">
																	{connection.scopes || "—"}
																</p>
																<p className="text-muted-foreground text-xs leading-5">
																	OAuth scope 快照，供排查当前连接能力。
																</p>
															</div>
															<div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-1">
																<div className="space-y-1.5 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
																	<p className="text-muted-foreground text-[11px] font-medium tracking-[0.16em] uppercase">
																		绑定时间
																	</p>
																	<p className="text-sm font-medium leading-6 text-foreground">
																		{formatDateTime(connection.linked_at)}
																	</p>
																</div>
																<div className="space-y-1.5 rounded-xl border border-border/70 bg-background/80 px-4 py-3">
																	<p className="text-muted-foreground text-[11px] font-medium tracking-[0.16em] uppercase">
																		最近刷新
																	</p>
																	<p className="text-sm font-medium leading-6 text-foreground">
																		{formatDateTime(connection.updated_at)}
																	</p>
																</div>
															</div>
														</div>
													</div>
												);
											})}
										</div>
									)}
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "linuxdo" ? (
						<section id="settings-linuxdo" data-settings-section="linuxdo">
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META.linuxdo.label}
											</CardTitle>
											<Badge variant={linuxdoStatusBadge.variant}>
												{linuxdoStatusBadge.label}
											</Badge>
										</div>
										{linuxdoConnection ? (
											<Button
												variant="outline"
												size="sm"
												disabled={linuxdoBusy}
												onClick={onDisconnectLinuxDo}
											>
												{linuxdoBusy ? (
													<LoaderCircle className="size-4 animate-spin" />
												) : (
													<Unlink2 className="size-4" />
												)}
												解绑 LinuxDO
											</Button>
										) : (
											<Button
												className="max-sm:min-h-11"
												size="sm"
												disabled={!linuxdoAvailable}
												onClick={onConnectLinuxDo}
											>
												<AuthProviderIcon provider="linuxdo" />
												绑定 LinuxDO
											</Button>
										)}
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									{linuxdoError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{linuxdoError}
										</div>
									) : null}

									{linuxdoLoading ? (
										<div className="text-muted-foreground flex items-center gap-2 text-sm">
											<LoaderCircle className="size-4 animate-spin" />
											正在读取 LinuxDO 绑定状态…
										</div>
									) : linuxdoConnection ? (
										<>
											<div className="flex min-w-0 items-center gap-3 rounded-2xl border border-border/70 bg-muted/20 p-4">
												{linuxdoConnection.avatar_url ? (
													<img
														src={linuxdoConnection.avatar_url}
														alt={`${linuxdoConnection.username} avatar`}
														className="size-12 rounded-full border border-border/70 object-cover"
														referrerPolicy="no-referrer"
													/>
												) : (
													<div className="bg-muted flex size-12 items-center justify-center rounded-full border border-border/70 text-xs font-semibold">
														LD
													</div>
												)}
												<div className="min-w-0">
													<p className="truncate text-sm font-semibold text-foreground">
														{linuxdoConnection.name ??
															linuxdoConnection.username}
													</p>
													<p className="text-muted-foreground truncate text-xs">
														@{linuxdoConnection.username}
													</p>
												</div>
											</div>

											<div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
												<DetailItem
													label="Trust level"
													value={linuxdoConnection.trust_level}
												/>
												<DetailItem
													label="账号状态"
													value={linuxdoConnection.active ? "正常" : "非活跃"}
												/>
												<DetailItem
													label="发言状态"
													value={linuxdoConnection.silenced ? "受限" : "正常"}
												/>
												<DetailItem
													label="绑定时间"
													value={formatDateTime(linuxdoConnection.linked_at)}
												/>
												<DetailItem
													label="更新时间"
													value={formatDateTime(linuxdoConnection.updated_at)}
												/>
											</div>
										</>
									) : (
										<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-foreground">
											未绑定 LinuxDO。
										</div>
									)}
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "my-releases" ? (
						<section
							id="settings-my-releases"
							data-settings-section="my-releases"
						>
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["my-releases"].label}
											</CardTitle>
											<Badge
												variant={includeOwnReleases ? "secondary" : "outline"}
											>
												{ownReleaseSummary}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={profileBusy}
											onClick={onSaveOwnReleases}
										>
											{ownReleaseSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<Package className="size-4" />
											)}
											保存“我的发布”
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									<div className="flex items-start justify-between gap-4 rounded-2xl border border-border/70 bg-muted/20 px-4 py-3">
										<div className="space-y-1.5">
											<p className="text-sm font-medium text-foreground">
												把自己的仓库发布也纳入发布流
											</p>
											<p className="text-muted-foreground text-sm leading-6">
												开启后，当前 GitHub 账号本人 owner 的仓库 release
												会像已加星仓库一样进入 “全部 /
												发布”、详情、翻译、润色与日报链路；真实“加星”列表和社交动态不会被污染。
											</p>
										</div>
										<Switch
											checked={includeOwnReleases}
											onCheckedChange={setIncludeOwnReleases}
											aria-label="我的发布"
											disabled={profileBusy}
										/>
									</div>

									{briefProfileError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{briefProfileError}
										</div>
									) : null}

									<div className="grid gap-3 sm:grid-cols-2">
										<DetailItem
											label="当前状态"
											value={
												includeOwnReleases
													? "已纳入我的发布"
													: "仅显示已加星仓库"
											}
										/>
										<DetailItem
											label="影响范围"
											value="发布列表 / Release 详情 / 翻译与润色 / 日报"
											hint="不影响真实加星列表，也不会新增社交事件。"
										/>
									</div>

									<div className="border-border/70 border-t pt-5">
										<div className="flex items-start justify-between gap-4">
											<div className="space-y-1.5">
												<div className="flex flex-wrap items-center gap-2">
													<p className="font-medium text-foreground text-sm">
														Webhook 推送
													</p>
													<Badge
														variant={
															webhookPush?.enabled ? "secondary" : "outline"
														}
													>
														{webhookPushLoading
															? "读取中"
															: webhookPush?.enabled
																? "已开启"
																: "已关闭"}
													</Badge>
												</div>
												<p className="text-muted-foreground text-sm leading-6">
													接收个人 owner 仓库的新 Release 通知，并进入现有
													Release 同步任务。
												</p>
											</div>
											<Switch
												checked={webhookPush?.enabled ?? false}
												onCheckedChange={onWebhookPushToggle}
												aria-label="Webhook 推送"
												disabled={
													webhookPushLoading ||
													webhookPushBusy === "toggle" ||
													(!includeOwnReleases && !webhookPush?.enabled)
												}
											/>
										</div>

										{webhookPushError ? (
											<div
												className={cn(
													"mt-4 rounded-lg border px-3 py-2.5 text-sm",
													statusToneClassName("error"),
												)}
											>
												<p>{webhookPushError}</p>
												{!webhookPush?.pat.configured ||
												!webhookPush?.pat.valid ? (
													<InternalLink
														className="mt-2 inline-flex font-medium underline underline-offset-4"
														href={buildSettingsHref("github-pat")}
														to={buildSettingsHref("github-pat")}
													>
														前往配置 GitHub PAT
													</InternalLink>
												) : null}
											</div>
										) : null}
										{webhookPushNotice ? (
											<div
												className={cn(
													"mt-4 rounded-lg border px-3 py-2.5 text-sm",
													statusToneClassName("success"),
												)}
											>
												{webhookPushNotice}
											</div>
										) : null}

										<div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
											<DetailItem
												label="已注册"
												value={String(webhookPush?.summary.registered ?? 0)}
											/>
											<DetailItem
												label="缺失"
												value={String(webhookPush?.summary.missing ?? 0)}
											/>
											<DetailItem
												label="权限暂停"
												value={String(
													webhookPush?.summary.permission_paused ?? 0,
												)}
											/>
											<DetailItem
												label="可清理"
												value={String(webhookPush?.summary.removable ?? 0)}
											/>
										</div>

										<div className="mt-4 grid gap-3 sm:grid-cols-2">
											<DetailItem
												label="最近定时巡查"
												value={
													webhookPush?.schedule.last_started_at
														? formatDateTime(
																webhookPush.schedule.last_started_at,
															)
														: "尚未执行"
												}
											/>
											<DetailItem
												label="下次定时巡查"
												value={
													webhookPush?.schedule.next_started_at
														? formatDateTime(
																webhookPush.schedule.next_started_at,
															)
														: `每 ${webhookPush?.schedule.audit_interval_days ?? 7} 天`
												}
												hint="权限暂停的仓库会跳过，直到手动注册成功。"
											/>
										</div>

										<div className="mt-4 flex flex-wrap gap-2">
											<Button
												className="max-sm:min-h-11"
												size="sm"
												variant="outline"
												disabled={
													!webhookPush?.enabled || webhookPushBusy !== null
												}
												onClick={() => void runWebhookPushAction("register")}
											>
												{webhookPushBusy === "register" ? (
													<LoaderCircle className="size-4 animate-spin" />
												) : (
													<Webhook className="size-4" />
												)}
												全量注册 Webhook
											</Button>
											<Button
												className="max-sm:min-h-11"
												size="sm"
												variant="outline"
												disabled={
													!webhookPush?.enabled || webhookPushBusy !== null
												}
												onClick={() => void runWebhookPushAction("check")}
											>
												{webhookPushBusy === "check" ? (
													<LoaderCircle className="size-4 animate-spin" />
												) : (
													<SearchCheck className="size-4" />
												)}
												全量检查 Webhook
											</Button>
											<Button
												className="max-sm:min-h-11"
												size="sm"
												variant="destructive"
												disabled={
													webhookPush?.enabled !== false ||
													(webhookPush?.summary.removable ?? 0) === 0 ||
													webhookPushBusy !== null
												}
												onClick={() => void runWebhookPushAction("delete")}
											>
												{webhookPushBusy === "delete" ? (
													<LoaderCircle className="size-4 animate-spin" />
												) : (
													<Trash2 className="size-4" />
												)}
												批量删除 Webhook
											</Button>
										</div>

										{webhookPush?.repos.length ? (
											<div className="mt-5 divide-y rounded-lg border">
												{webhookPush.repos.map((repo) => (
													<div
														className="flex flex-col gap-3 p-3 sm:flex-row sm:items-center sm:justify-between"
														key={repo.repo_id}
													>
														<div className="min-w-0 space-y-1">
															<div className="flex flex-wrap items-center gap-2">
																<p className="truncate font-medium text-sm">
																	{repo.repo_full_name}
																</p>
																<Badge
																	variant={
																		repo.permission_paused
																			? "destructive"
																			: repo.status === "registered"
																				? "secondary"
																				: "outline"
																	}
																>
																	{repo.permission_paused
																		? "权限暂停"
																		: repo.status === "delete_pending"
																			? "删除中"
																			: repo.status === "registered"
																				? "已注册"
																				: repo.status === "missing"
																					? "缺失"
																					: "异常"}
																</Badge>
															</div>
															{repo.error_message ? (
																<p className="text-destructive text-sm">
																	{repo.error_message}
																</p>
															) : null}
														</div>
														<div className="flex shrink-0 gap-2">
															<Button
																className="max-sm:min-h-11"
																size="sm"
																variant="outline"
																disabled={
																	!webhookPush.enabled ||
																	webhookPushBusy !== null
																}
																onClick={() =>
																	void runWebhookPushAction(
																		"register",
																		repo.repo_id,
																	)
																}
															>
																注册
															</Button>
															<Button
																size="sm"
																variant="ghost"
																disabled={
																	!webhookPush.enabled ||
																	webhookPushBusy !== null
																}
																onClick={() =>
																	void runWebhookPushAction(
																		"check",
																		repo.repo_id,
																	)
																}
															>
																检查
															</Button>
														</div>
													</div>
												))}
											</div>
										) : null}
									</div>
								</CardContent>
							</Card>

							<AlertDialog
								open={webhookPushConfirmOpen}
								onOpenChange={setWebhookPushConfirmOpen}
							>
								<AlertDialogContent>
									<AlertDialogHeader>
										<AlertDialogTitle>开启 Webhook 推送？</AlertDialogTitle>
										<AlertDialogDescription className="space-y-3 text-left leading-6">
											<span className="block">
												OctoRill 会使用当前 classic PAT 的 <strong>repo</strong>{" "}
												或 <strong>public_repo</strong> 权限，在 PAT
												所属账号的个人 owner 仓库中创建、检查和修复仅监听
												Release 的 webhook。
											</span>
											<span className="block">
												每个 webhook 使用独立的加密 secret 验证 GitHub
												签名，只在新 Release 发布时排队同步。关闭后现有 hooks
												会保留，但事件将被忽略；需要时可在关闭后批量删除
												OctoRill 创建的 hooks。
											</span>
										</AlertDialogDescription>
									</AlertDialogHeader>
									<AlertDialogFooter>
										<AlertDialogCancel disabled={webhookPushBusy === "toggle"}>
											取消
										</AlertDialogCancel>
										<AlertDialogAction
											disabled={webhookPushBusy === "toggle"}
											onClick={(event) => {
												event.preventDefault();
												confirmWebhookPushEnable();
											}}
										>
											{webhookPushBusy === "toggle" ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<Webhook className="size-4" />
											)}
											确认开启并注册
										</AlertDialogAction>
									</AlertDialogFooter>
								</AlertDialogContent>
							</AlertDialog>
						</section>
					) : null}

					{section === "github-pat" ? (
						<section
							id="settings-github-pat"
							data-settings-section="github-pat"
						>
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["github-pat"].label}
											</CardTitle>
											<Badge variant={patStatusBadge.variant}>
												{patStatusBadge.label}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={patSaving || !canSavePat}
											onClick={onSavePat}
										>
											{patSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<KeyRound className="size-4" />
											)}
											保存 GitHub PAT
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									<div className="space-y-2">
										<Label htmlFor="settings-reaction-pat">GitHub PAT</Label>
										<GitHubPatInput
											id="settings-reaction-pat"
											value={patInput}
											onChange={(event) => setPatInput(event.target.value)}
											placeholder="粘贴新的 classic PAT"
											autoCapitalize="none"
											autoCorrect="off"
											spellCheck={false}
											inputClassName="h-10 font-mono text-sm"
										/>
									</div>

									<div
										className={cn(
											"rounded-xl border px-3 py-2.5 text-sm",
											statusToneClassName(patTone),
										)}
									>
										<p className="font-medium">
											{patCheckState === "checking"
												? "正在校验 GitHub PAT"
												: patCheckState === "valid"
													? "GitHub PAT 可用"
													: patCheckState === "invalid"
														? "GitHub PAT 无效"
														: patCheckState === "error"
															? "GitHub PAT 校验失败"
															: reactionTokenConfigured
																? "已保存 GitHub PAT"
																: "尚未填写新的 GitHub PAT"}
										</p>
										<p className="mt-1 text-xs leading-5">
											{patCheckMessage ??
												"输入后会在 800ms 后自动校验；只有 valid 时允许保存。"}
										</p>
									</div>

									<div className="grid gap-3 sm:grid-cols-2">
										<DetailItem
											label="当前保存"
											value={
												reactionTokenLoading
													? "读取中…"
													: (reactionTokenMasked ?? "未配置")
											}
										/>
										<DetailItem
											label="最近检查"
											value={patCheckedAt ? formatDateTime(patCheckedAt) : "—"}
										/>
										<DetailItem
											label="PAT 归属"
											value={
												reactionTokenOwner
													? `@${reactionTokenOwner.login}`
													: "未配置"
											}
											hint="PAT 只能属于当前账号下某个已绑定 GitHub 账号。"
										/>
									</div>

									<GitHubPatGuideCard compact />
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "api-keys" ? (
						<section id="settings-api-keys" data-settings-section="api-keys">
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["api-keys"].label}
											</CardTitle>
											<Badge variant={apiKeyStatusBadge.variant}>
												{apiKeyStatusBadge.label}
											</Badge>
										</div>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									{apiKeysError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{apiKeysError}
										</div>
									) : null}

									<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-muted-foreground">
										API Key
										只能调用发布、Feed、通知、日报、同步、翻译与任务流等用户态业务接口；账号设置、凭据管理、登录与管理员接口仍需网页登录。
										列表会显示完整 Key，请只在可信设备上打开本页。
									</div>

									<form
										className="grid gap-3 rounded-2xl border border-border/70 bg-background/80 p-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end"
										onSubmit={onCreateApiKey}
									>
										<div className="space-y-2">
											<Label htmlFor="settings-api-key-name">名称</Label>
											<Input
												id="settings-api-key-name"
												value={apiKeyName}
												maxLength={80}
												onChange={(event) => setApiKeyName(event.target.value)}
												placeholder="例如：local sync script"
												autoComplete="off"
											/>
										</div>
										<Button type="submit" disabled={apiKeysCreating}>
											{apiKeysCreating ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<Plus className="size-4" />
											)}
											创建 API Key
										</Button>
									</form>

									{createdApiKey ? (
										<div
											className="space-y-3 rounded-2xl border border-emerald-500/30 bg-emerald-500/10 p-4"
											data-api-key-created
										>
											<div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
												<div className="space-y-1">
													<p className="text-sm font-semibold text-foreground">
														API Key 已创建
													</p>
													<p className="text-muted-foreground text-sm leading-6">
														已加入下方列表，之后仍可在本页查看和复制完整 Key。
													</p>
												</div>
												<Button
													type="button"
													size="sm"
													variant="outline"
													onClick={() =>
														onCopyApiKey(
															createdApiKey.item.id,
															createdApiKey.api_key,
														)
													}
												>
													<Copy className="size-4" />
													{copiedApiKeyId === createdApiKey.item.id
														? "已复制"
														: "复制"}
												</Button>
											</div>
											<div className="rounded-xl border border-emerald-500/25 bg-background/90 px-3 py-2 font-mono text-sm break-all text-foreground">
												{createdApiKey.api_key}
											</div>
										</div>
									) : null}

									{apiKeysLoading ? (
										<div className="text-muted-foreground flex items-center gap-2 text-sm">
											<LoaderCircle className="size-4 animate-spin" />
											正在读取 API Key 列表…
										</div>
									) : apiKeys.length === 0 ? (
										<div className="rounded-2xl border border-border/70 bg-background/80 px-4 py-4 text-sm leading-6 text-foreground">
											<p className="font-medium">还没有 API Key。</p>
											<p className="text-muted-foreground mt-1">
												创建后可用 Authorization: Bearer
												调用允许的用户态业务接口。
											</p>
										</div>
									) : (
										<div className="space-y-3">
											{apiKeys.map((apiKey) => {
												const isBusy = apiKeysBusyId === apiKey.id;
												return (
													<div
														key={apiKey.id}
														className="space-y-3 rounded-2xl border border-border/70 bg-background/80 p-4 sm:p-5"
														data-api-key-item={apiKey.id}
													>
														<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
															<div className="min-w-0 space-y-1.5">
																<p className="truncate text-base font-semibold text-foreground">
																	{apiKey.name}
																</p>
																<p className="font-mono text-sm break-all text-muted-foreground">
																	{apiKey.api_key}
																</p>
															</div>
															<div className="flex shrink-0 flex-wrap gap-2">
																<Button
																	type="button"
																	variant="outline"
																	size="sm"
																	onClick={() =>
																		onCopyApiKey(apiKey.id, apiKey.api_key)
																	}
																>
																	<Copy className="size-4" />
																	{copiedApiKeyId === apiKey.id
																		? "已复制"
																		: "复制"}
																</Button>
																<AlertDialog
																	open={pendingApiKeyRevokeId === apiKey.id}
																	onOpenChange={(open) =>
																		setPendingApiKeyRevokeId(
																			open ? apiKey.id : null,
																		)
																	}
																>
																	<AlertDialogTrigger asChild>
																		<Button
																			type="button"
																			variant="outline"
																			size="sm"
																			disabled={isBusy}
																		>
																			{isBusy ? (
																				<LoaderCircle className="size-4 animate-spin" />
																			) : (
																				<Trash2 className="size-4" />
																			)}
																			撤销
																		</Button>
																	</AlertDialogTrigger>
																	<AlertDialogContent>
																		<AlertDialogHeader>
																			<AlertDialogTitle>
																				确认撤销 API Key
																			</AlertDialogTitle>
																			<AlertDialogDescription>
																				撤销后 {apiKey.name} 会立即失效，使用该
																				Key
																				的脚本和外部客户端将无法继续调用接口。
																			</AlertDialogDescription>
																		</AlertDialogHeader>
																		<AlertDialogFooter>
																			<AlertDialogCancel>
																				取消
																			</AlertDialogCancel>
																			<AlertDialogAction
																				onClick={() =>
																					onDeleteApiKey(apiKey.id)
																				}
																			>
																				确认撤销
																			</AlertDialogAction>
																		</AlertDialogFooter>
																	</AlertDialogContent>
																</AlertDialog>
															</div>
														</div>
														<div className="grid gap-3 sm:grid-cols-2">
															<DetailItem
																label="创建时间"
																value={formatDateTime(apiKey.created_at)}
															/>
															<DetailItem
																label="最近使用"
																value={formatDateTime(apiKey.last_used_at)}
																hint={`掩码：${apiKey.masked_key}`}
															/>
														</div>
													</div>
												);
											})}
										</div>
									)}
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "daily-brief" ? (
						<section
							id="settings-daily-brief"
							data-settings-section="daily-brief"
						>
							<Card className="border-border/70 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
								<CardHeader className="border-b border-border/60 p-5 max-md:border-b-0 max-md:px-0 max-md:pb-4 max-md:pt-0">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["daily-brief"].label}
											</CardTitle>
											<Badge variant="outline">{briefSummary}</Badge>
										</div>
										<Button
											size="sm"
											disabled={profileBusy}
											onClick={onSaveBriefProfile}
										>
											{briefProfileSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<CalendarClock className="size-4" />
											)}
											保存日报设置
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5 max-md:px-0 max-md:pb-0">
									<DailyBriefProfileForm
										timeZone={briefProfileDraft.daily_brief_time_zone}
										disabled={profileBusy}
										error={briefProfileError}
										compact
										helperText="日报会按这里保存的时区解释“昨天”自然日。"
										onTimeZoneChange={(value) =>
											setBriefProfileDraft((current) => ({
												...current,
												daily_brief_time_zone: value,
											}))
										}
										onUseBrowserTimeZone={(timeZone) =>
											setBriefProfileDraft((current) => ({
												...current,
												daily_brief_time_zone: timeZone,
											}))
										}
									/>
								</CardContent>
							</Card>
						</section>
					) : null}
				</div>
			</div>
		</AppShell>
	);
}
