import { useEffect } from "react";
import { useRouter } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";

export function useRequiredAdmin() {
	const auth = useAuthBootstrap();
	const router = useRouter();

	useEffect(() => {
		if (auth.status === "pending") return;
		if (auth.isBootstrapping && auth.bootPresentation !== "live") return;
		if (auth.isAuthenticated && auth.me?.user.is_admin) return;
		void router.navigate({
			to: "/",
			search: {
				tab: undefined,
				release: undefined,
			},
			replace: true,
		});
	}, [
		auth.bootPresentation,
		auth.isAuthenticated,
		auth.isBootstrapping,
		auth.me?.user.is_admin,
		auth.status,
		router,
	]);

	if (!auth.isAuthenticated || !auth.me?.user.is_admin) {
		return null;
	}

	return auth.me;
}
