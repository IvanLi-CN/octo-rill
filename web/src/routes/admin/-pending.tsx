import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	AdminDashboardStartupSkeleton,
	AdminJobsStartupSkeleton,
	AdminUsersStartupSkeleton,
	AppBoot,
} from "@/pages/AppBoot";

type AdminPendingVariant = "dashboard" | "users" | "jobs";

export function AdminRoutePending(props: { variant: AdminPendingVariant }) {
	const { variant } = props;
	const auth = useAuthBootstrap();

	if (!auth.isAuthenticated || !auth.me?.user.is_admin) {
		return <AppBoot />;
	}

	switch (variant) {
		case "users":
			return <AdminUsersStartupSkeleton me={auth.me} />;
		case "jobs":
			return <AdminJobsStartupSkeleton me={auth.me} />;
		default:
			return <AdminDashboardStartupSkeleton me={auth.me} />;
	}
}
