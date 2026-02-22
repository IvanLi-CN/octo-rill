import { useEffect, useState } from "react";

import { ApiError, apiGet } from "@/api";
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
	};
};

function App() {
	const [me, setMe] = useState<MeResponse | null>(null);
	const [bootError, setBootError] = useState<string | null>(null);

	const isLoggedIn = Boolean(me?.user?.id);

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

	if (!isLoggedIn) {
		return <Landing bootError={bootError} />;
	}

	return <Dashboard me={me!} />;
}

export default App;
