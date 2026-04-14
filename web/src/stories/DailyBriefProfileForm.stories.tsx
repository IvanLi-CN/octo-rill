import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { DailyBriefProfileForm } from "@/briefs/DailyBriefProfileForm";

function Preview(args: {
	localTime: string;
	timeZone: string;
	error?: string | null;
	helperText?: string;
	disabled?: boolean;
}) {
	const [localTime, setLocalTime] = useState(args.localTime);
	const [timeZone, setTimeZone] = useState(args.timeZone);
	return (
		<div className="max-w-xl rounded-xl border bg-background p-6">
			<DailyBriefProfileForm
				localTime={localTime}
				timeZone={timeZone}
				error={args.error}
				helperText={args.helperText}
				disabled={args.disabled}
				onLocalTimeChange={setLocalTime}
				onTimeZoneChange={setTimeZone}
				onUseBrowserTimeZone={setTimeZone}
			/>
		</div>
	);
}

const meta = {
	title: "Dashboard/Daily Brief Profile Form",
	component: Preview,
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"日报设置表单，统一承载普通用户自助设置与管理员代他人编辑。时间只允许整点，时区必须是 IANA 名称。",
			},
		},
	},
	args: {
		localTime: "08:00",
		timeZone: "Asia/Shanghai",
		helperText:
			"保存后只影响未来生成的日报；已经生成的历史快照仍按原窗口解释。",
		error: null,
		disabled: false,
	},
} satisfies Meta<typeof Preview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const InvalidTimeZone: Story = {
	args: {
		timeZone: "UTC+8",
		error: "invalid daily brief time zone (expected IANA time zone)",
		helperText:
			"服务端不会把非法时区偷偷降级成 offset-only；必须保存合法 IANA 名称。",
	},
};

export const DstAwareWindow: Story = {
	args: {
		localTime: "09:00",
		timeZone: "America/New_York",
		helperText:
			"DST 切换日也按 IANA 规则求窗口：歧义时取第一次出现，不存在时顺延到下一个有效时刻。",
	},
};
