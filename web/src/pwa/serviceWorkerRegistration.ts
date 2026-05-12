type PwaServiceWorkerUpdateController = {
	applyUpdate: () => void;
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
let registered = false;
let reloadingForControllerChange = false;
let shouldReloadOnControllerChange = false;

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

				const notify = (worker: ServiceWorker) => {
					options.onNeedRefresh({
						applyUpdate: () => {
							shouldReloadOnControllerChange = true;
							worker.postMessage({ type: "SKIP_WAITING" });
						},
					});
				};

				if (registration.waiting) {
					notify(registration.waiting);
				}

				registration.addEventListener("updatefound", () => {
					const installingWorker = registration.installing;
					if (!installingWorker) return;

					installingWorker.addEventListener("statechange", () => {
						if (
							installingWorker.state === "installed" &&
							navigator.serviceWorker.controller
						) {
							notify(installingWorker);
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
		window.location.reload();
	});
}
