import { useEffect, useState } from "react";

import { ApiError, apiGet } from "@/api";
import { AdminPanel } from "@/pages/AdminPanel";
import { Dashboard } from "@/pages/Dashboard";
import { Landing } from "@/pages/Landing";

type MeResponse = {
	user: {
		id: number;
		github_user_id: number;
		login: string;
		name: string | null;
		avatar_url: string | null;
		email: string | null;
		is_admin: boolean;
	};
};

function App() {
	const [me, setMe] = useState<MeResponse | null>(null);
	const [bootError, setBootError] = useState<string | null>(null);

	const isLoggedIn = Boolean(me?.user?.id);
	const isAdminRoute = window.location.pathname === "/admin";

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
		if (!isLoggedIn || !isAdminRoute) return;
		if (me?.user.is_admin) return;
		window.history.replaceState({}, "", "/");
	}, [isAdminRoute, isLoggedIn, me?.user.is_admin]);

	if (!isLoggedIn || !me) {
		return <Landing bootError={bootError} />;
	}

	if (isAdminRoute && me.user.is_admin) {
		return <AdminPanel me={me} />;
	}

	return <Dashboard me={me} />;
}

export default App;
