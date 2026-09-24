type PwaServiceWorkerUpdateController = {
	applyUpdate: () => Promise<boolean>;
};

type PwaServiceWorkerRegistrationController = {
	checkForUpdate: () => void;
};

type RegisterPwaServiceWorkerOptions = {
	onNeedRefresh: (controller: PwaServiceWorkerUpdateController) => void;
	onRegistered?: (controller: PwaServiceWorkerRegistrationController) => void;
	onRegisterError?: (error: unknown) => void;
};

const SW_URL = "/sw.js";
const ACTIVATION_TIMEOUT_MS = 15_000;
let registered = false;
let reloadingForControllerChange = false;
let shouldReloadOnControllerChange = false;
let pendingActivation: {
	promise: Promise<boolean>;
	resolve: (activated: boolean) => void;
	timeoutId: number;
} | null = null;

function finishActivation(activated: boolean) {
	const attempt = pendingActivation;
	if (!attempt) return;

	pendingActivation = null;
	window.clearTimeout(attempt.timeoutId);
	attempt.resolve(activated);
}

function applyWaitingWorker(registration: ServiceWorkerRegistration) {
	if (pendingActivation) return pendingActivation.promise;

	const worker = registration.waiting;
	if (!worker) {
		window.location.reload();
		return Promise.resolve(true);
	}

	shouldReloadOnControllerChange = true;
	let timeoutId = 0;
	let resolveAttempt!: (activated: boolean) => void;
	const promise = new Promise<boolean>((resolve) => {
		resolveAttempt = resolve;
		timeoutId = window.setTimeout(() => {
			if (pendingActivation?.promise !== promise) return;
			shouldReloadOnControllerChange = false;
			finishActivation(false);
		}, ACTIVATION_TIMEOUT_MS);
	});
	pendingActivation = { promise, resolve: resolveAttempt, timeoutId };

	try {
		worker.postMessage({ type: "SKIP_WAITING" });
	} catch {
		shouldReloadOnControllerChange = false;
		finishActivation(false);
	}

	return promise;
}

function isPwaServiceWorkerSupported() {
	return (
		typeof window !== "undefined" &&
		"serviceWorker" in navigator &&
		window.location.protocol !== "file:"
	);
}

export function registerPwaServiceWorker(
	options: RegisterPwaServiceWorkerOptions,
) {
	if (!isPwaServiceWorkerSupported() || registered) {
		return;
	}
	registered = true;

	const register = () => {
		void navigator.serviceWorker
			.register(SW_URL)
			.then((registration) => {
				options.onRegistered?.({
					checkForUpdate: () => {
						void registration.update().catch((error: unknown) => {
							options.onRegisterError?.(error);
						});
					},
				});

				const notify = () => {
					options.onNeedRefresh({
						applyUpdate: () => applyWaitingWorker(registration),
					});
				};

				if (registration.waiting) {
					notify();
				}

				registration.addEventListener("updatefound", () => {
					const installingWorker = registration.installing;
					if (!installingWorker) return;

					installingWorker.addEventListener("statechange", () => {
						if (
							installingWorker.state === "installed" &&
							navigator.serviceWorker.controller
						) {
							notify();
						}
					});
				});
			})
			.catch((error: unknown) => {
				options.onRegisterError?.(error);
			});
	};

	if (document.readyState === "complete") {
		register();
	} else {
		window.addEventListener("load", register, { once: true });
	}

	navigator.serviceWorker.addEventListener("controllerchange", () => {
		if (!shouldReloadOnControllerChange) return;
		if (reloadingForControllerChange) return;
		reloadingForControllerChange = true;
		shouldReloadOnControllerChange = false;
		finishActivation(true);
		window.location.reload();
	});
}
