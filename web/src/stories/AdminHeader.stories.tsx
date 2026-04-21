import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import { AdminHeader } from "@/layout/AdminHeader";

const ADMIN_HEADER_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	adminHeaderNarrowTablet640: {
		name: "Admin header narrow tablet 640x960",
		styles: {
			height: "960px",
			width: "640px",
		},
		type: "tablet",
	},
	adminHeaderNarrowTablet757: {
		name: "Admin header narrow tablet 757x827",
		styles: {
			height: "827px",
			width: "757px",
		},
		type: "tablet",
	},
	adminHeaderTablet853: {
		name: "Admin header tablet 853x1280",
		styles: {
			height: "1280px",
			width: "853px",
		},
		type: "tablet",
	},
	adminHeaderTablet1023: {
		name: "Admin header tablet 1023x1280",
		styles: {
			height: "1280px",
			width: "1023px",
		},
		type: "tablet",
	},
} as const;

const LONG_LOGIN = "storybook-admin-with-a-very-long-login-name";

function expectTabletInlineLayout(options: {
	mainRow: HTMLElement | null;
	leadingBlock: HTMLElement | null;
	trailingBlock: HTMLElement | null;
	loginLabel?: HTMLElement | null;
}) {
	const { mainRow, leadingBlock, trailingBlock, loginLabel } = options;
	expect(mainRow).not.toBeNull();
	expect(leadingBlock).not.toBeNull();
	expect(trailingBlock).not.toBeNull();
	if (!mainRow || !leadingBlock || !trailingBlock) {
		throw new Error("Expected main row, leading block, and trailing block");
	}

	const mainRect = mainRow.getBoundingClientRect();
	const leadingRect = leadingBlock.getBoundingClientRect();
	const trailingRect = trailingBlock.getBoundingClientRect();
	expect(mainRow.scrollWidth - mainRow.clientWidth).toBeLessThanOrEqual(1);
	expect(trailingRect.top - mainRect.top).toBeLessThanOrEqual(12);
	expect(trailingRect.top - leadingRect.top).toBeLessThanOrEqual(12);
	expect(trailingRect.right).toBeLessThanOrEqual(mainRect.right + 1);
	expect(mainRect.right - trailingRect.right).toBeLessThanOrEqual(12);
	if (loginLabel) {
		const loginRect = loginLabel.getBoundingClientRect();
		expect(loginRect.right).toBeLessThanOrEqual(mainRect.right + 1);
	}
}

const meta = {
	title: "Layout/Admin Header",
	component: AdminHeader,
	tags: ["autodocs"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: ADMIN_HEADER_VIEWPORTS,
		},
		docs: {
			description: {
				component:
					"AdminHeader 复用 Dashboard 的品牌语义与移动壳层状态，并在平板与桌面区间维持品牌 / 导航 / utility actions 的清晰分栏。",
			},
		},
	},
	args: {
		activeNav: "jobs",
		user: {
			login: "storybook-admin",
		},
	},
	render: (args) => (
		<div className="bg-background p-6">
			<section className="rounded-3xl border bg-card p-5 shadow-sm">
				<AdminHeader {...args} />
			</section>
		</div>
	),
} satisfies Meta<typeof AdminHeader>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("navigation", { name: "管理员导航" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "返回前台首页" }),
		).toBeVisible();
		await expect(canvas.getByText("storybook-admin")).toBeVisible();
	},
	parameters: {
		docs: {
			description: {
				story:
					"默认管理后台页头：验证品牌、导航与 utility actions 的基础层级与可达性。",
			},
		},
	},
};

export const EvidenceTabletInline: Story = {
	name: "Evidence / Tablet Inline Header",
	args: {
		user: {
			login: LONG_LOGIN,
		},
	},
	globals: {
		viewport: {
			value: "adminHeaderTablet853",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("navigation", { name: "管理员导航" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "返回前台首页" }),
		).toBeVisible();
		expectTabletInlineLayout({
			mainRow: canvasElement.querySelector<HTMLElement>(
				"[data-admin-header-main-row]",
			),
			leadingBlock: canvasElement.querySelector<HTMLElement>(
				"[data-admin-nav-block]",
			),
			trailingBlock: canvasElement.querySelector<HTMLElement>(
				"[data-admin-primary-actions]",
			),
			loginLabel: canvasElement.querySelector<HTMLElement>(
				"[data-admin-login-label]",
			),
		});
	},
	parameters: {
		docs: {
			description: {
				story:
					"平板 853x1280 证据入口：utility actions 必须固定在品牌 / 导航块右侧，长 login 只能截断自身，不能把 `返回前台` 挤出视口。",
			},
		},
	},
};

export const RegressionNarrowTablet640: Story = {
	name: "Regression / Narrow Tablet 640",
	args: {
		user: {
			login: LONG_LOGIN,
		},
	},
	globals: {
		viewport: {
			value: "adminHeaderNarrowTablet640",
			isRotated: false,
		},
	},
	play: EvidenceTabletInline.play,
	parameters: {
		docs: {
			description: {
				story:
					"640x960 回归入口：窄平板起点必须保持 utility actions 与品牌 / 导航块同排，长 login 只允许自身截断。",
			},
		},
	},
};

export const RegressionNarrowTablet757: Story = {
	name: "Regression / Narrow Tablet 757",
	args: {
		user: {
			login: LONG_LOGIN,
		},
	},
	globals: {
		viewport: {
			value: "adminHeaderNarrowTablet757",
			isRotated: false,
		},
	},
	play: EvidenceTabletInline.play,
	parameters: {
		docs: {
			description: {
				story:
					"757x827 主回归入口：对应窄平板主复现宽度，`返回前台` 与退出动作不能被长 login 挤出可视区。",
			},
		},
	},
};

export const RegressionTablet1023: Story = {
	name: "Regression / Tablet 1023",
	args: {
		user: {
			login: LONG_LOGIN,
		},
	},
	globals: {
		viewport: {
			value: "adminHeaderTablet1023",
			isRotated: false,
		},
	},
	play: EvidenceTabletInline.play,
	parameters: {
		docs: {
			description: {
				story:
					"1023x1280 上边界入口：在进入桌面语义前，Admin utility cluster 仍需稳定贴在右列。",
			},
		},
	},
};
