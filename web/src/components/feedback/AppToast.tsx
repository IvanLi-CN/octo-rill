import {
	createContext,
	useCallback,
	useContext,
	useMemo,
	useState,
} from "react";

import { ErrorDetailDisclosure } from "@/components/feedback/ErrorDetailDisclosure";
import { Button } from "@/components/ui/button";
import {
	Toast,
	ToastAction,
	ToastClose,
	ToastDescription,
	ToastProvider,
	ToastTitle,
	ToastViewport,
} from "@/components/ui/toast";
import { useAppShellChrome } from "@/layout/AppShell";

type ToastVariant = "default" | "destructive";

type AppToastRecord = {
	id: string;
	title: string;
	description: string;
	detail?: string | null;
	variant: ToastVariant;
	duration: number;
	actionLabel?: string;
	onAction?: () => void;
	open: boolean;
};

export type AppToastInput = {
	title: string;
	description: string;
	detail?: string | null;
	variant?: ToastVariant;
	duration?: number;
	actionLabel?: string;
	onAction?: () => void;
};

type AppToastContextValue = {
	toasts: AppToastRecord[];
	pushToast: (toast: AppToastInput) => string;
	pushErrorToast: (
		title: string,
		description: string,
		options?: Omit<AppToastInput, "title" | "description" | "variant">,
	) => string;
	dismissToast: (id: string) => void;
	removeToast: (id: string) => void;
};

const AppToastContext = createContext<AppToastContextValue | null>(null);

function makeToastId() {
	if (
		typeof crypto !== "undefined" &&
		typeof crypto.randomUUID === "function"
	) {
		return crypto.randomUUID();
	}
	return `toast-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function AppToastProvider(props: { children: React.ReactNode }) {
	const { children } = props;
	const [toasts, setToasts] = useState<AppToastRecord[]>([]);

	const dismissToast = useCallback((id: string) => {
		setToasts((current) =>
			current.map((toast) =>
				toast.id === id ? { ...toast, open: false } : toast,
			),
		);
		window.setTimeout(() => {
			setToasts((current) => current.filter((toast) => toast.id !== id));
		}, 220);
	}, []);

	const removeToast = useCallback((id: string) => {
		setToasts((current) => current.filter((toast) => toast.id !== id));
	}, []);

	const pushToast = useCallback((input: AppToastInput) => {
		const id = makeToastId();
		setToasts((current) => [
			{
				id,
				title: input.title,
				description: input.description,
				detail: input.detail ?? null,
				variant: input.variant ?? "default",
				duration: input.duration ?? 6000,
				actionLabel: input.actionLabel,
				onAction: input.onAction,
				open: true,
			},
			...current,
		]);
		return id;
	}, []);

	const pushErrorToast = useCallback(
		(
			title: string,
			description: string,
			options?: Omit<AppToastInput, "title" | "description" | "variant">,
		) => {
			return pushToast({
				title,
				description,
				detail: options?.detail,
				duration: options?.duration,
				actionLabel: options?.actionLabel,
				onAction: options?.onAction,
				variant: "destructive",
			});
		},
		[pushToast],
	);

	const value = useMemo<AppToastContextValue>(
		() => ({
			toasts,
			pushToast,
			pushErrorToast,
			dismissToast,
			removeToast,
		}),
		[toasts, pushToast, pushErrorToast, dismissToast, removeToast],
	);

	return (
		<ToastProvider swipeDirection="right">
			<AppToastContext.Provider value={value}>
				{children}
			</AppToastContext.Provider>
		</ToastProvider>
	);
}

export function useAppToast() {
	const context = useContext(AppToastContext);
	if (!context) {
		throw new Error("useAppToast must be used within AppToastProvider");
	}
	return context;
}

export function AppToastViewportHost() {
	const { toasts, dismissToast } = useAppToast();
	const { headerHeight } = useAppShellChrome();
	const topOffset = headerHeight > 0 ? headerHeight + 12 : 16;

	return (
		<>
			{toasts.map((toast) => (
				<Toast
					key={toast.id}
					open={toast.open}
					onOpenChange={(open) => {
						if (!open) {
							dismissToast(toast.id);
						}
					}}
					variant={toast.variant}
					duration={toast.duration}
					data-variant={toast.variant}
				>
					<div className="min-w-0 flex-1 space-y-1.5 pr-6">
						<ToastTitle>{toast.title}</ToastTitle>
						<ToastDescription>{toast.description}</ToastDescription>
						<ErrorDetailDisclosure
							detail={toast.detail}
							summary={toast.description}
						/>
						{toast.actionLabel && toast.onAction ? (
							<div className="pt-1">
								<ToastAction altText={toast.actionLabel} asChild>
									<Button
										variant="outline"
										size="sm"
										className="h-8 rounded-full font-mono text-xs"
										onClick={toast.onAction}
									>
										{toast.actionLabel}
									</Button>
								</ToastAction>
							</div>
						) : null}
					</div>
					<ToastClose />
				</Toast>
			))}
			<ToastViewport
				style={{
					top: `calc(env(safe-area-inset-top, 0px) + ${topOffset}px)`,
				}}
			/>
		</>
	);
}
