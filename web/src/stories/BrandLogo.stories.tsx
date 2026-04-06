import { BrandLogo } from "@/components/brand/BrandLogo";
import { AdminHeader } from "@/layout/AdminHeader";
import { DashboardHeader } from "@/pages/DashboardHeader";
import type { Meta, StoryObj } from "@storybook/react-vite";

function SurfaceGallery() {
	return (
		<div className="bg-background text-foreground grid gap-6 p-4">
			<section className="rounded-2xl border bg-card p-4 shadow-sm">
				<p className="text-muted-foreground mb-4 text-xs font-medium uppercase tracking-[0.2em]">
					Standalone assets
				</p>
				<div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
					<div className="rounded-2xl border bg-[#FFF8EE] p-4">
						<p className="text-muted-foreground mb-3 text-[11px] font-medium uppercase tracking-[0.2em]">
							Mark
						</p>
						<div className="flex h-24 items-center justify-center rounded-2xl bg-white/60 p-3">
							<BrandLogo variant="mark" className="h-full" />
						</div>
					</div>

					<div className="rounded-2xl border bg-background p-4">
						<p className="text-muted-foreground mb-3 text-[11px] font-medium uppercase tracking-[0.2em]">
							Wordmark light
						</p>
						<div className="flex h-24 items-center rounded-2xl bg-[#FFF8EE] px-4">
							<BrandLogo theme="light" variant="wordmark" className="h-10" />
						</div>
					</div>

					<div className="rounded-2xl border bg-background p-4">
						<p className="text-muted-foreground mb-3 text-[11px] font-medium uppercase tracking-[0.2em]">
							Wordmark dark
						</p>
						<div className="flex h-24 items-center rounded-2xl bg-[#252A30] px-4">
							<BrandLogo theme="dark" variant="wordmark" className="h-10" />
						</div>
					</div>

					<div className="rounded-2xl border bg-background p-4">
						<p className="text-muted-foreground mb-3 text-[11px] font-medium uppercase tracking-[0.2em]">
							Favicon icon
						</p>
						<div className="flex h-24 items-center justify-center rounded-2xl bg-[#252A30] p-3">
							<img
								alt="OctoRill favicon preview"
								className="size-16"
								src="/brand/favicon.svg"
							/>
						</div>
					</div>
				</div>
			</section>

			<section className="rounded-2xl border bg-card p-4 shadow-sm">
				<p className="text-muted-foreground mb-4 text-xs font-medium uppercase tracking-[0.2em]">
					Landing badge
				</p>
				<div className="inline-flex items-center gap-2 rounded-full border bg-card px-3 py-1 text-xs font-medium">
					<BrandLogo variant="wordmark" className="h-5" />
					<span className="text-muted-foreground">
						GitHub 信息流 · 中文翻译
					</span>
				</div>
			</section>

			<section className="rounded-2xl border bg-card p-4 shadow-sm">
				<p className="text-muted-foreground mb-4 text-xs font-medium uppercase tracking-[0.2em]">
					Dashboard header
				</p>
				<DashboardHeader
					briefCount={6}
					busy={false}
					feedCount={24}
					inboxCount={8}
					isAdmin
					login="storybook-admin"
					logoutHref="#"
				/>
			</section>

			<section className="rounded-2xl border bg-card p-4 shadow-sm">
				<p className="text-muted-foreground mb-4 text-xs font-medium uppercase tracking-[0.2em]">
					Admin header
				</p>
				<AdminHeader
					activeNav="jobs"
					user={{
						login: "storybook-admin",
					}}
				/>
			</section>
		</div>
	);
}

const meta = {
	title: "Brand/Logo",
	component: BrandLogo,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"OctoRill 的品牌主标：用更可爱的小章鱼抱卡片形象承担 favicon、站点字标与页面品牌露出。该 stories 同时提供独立资产预览与 Landing / Dashboard / Admin 场景对照，便于后续稳定截图。",
			},
		},
	},
} satisfies Meta<typeof BrandLogo>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Wordmark: Story = {
	args: {
		variant: "wordmark",
	},
	render: (args) => (
		<div className="bg-background grid gap-4 p-6">
			<div className="rounded-2xl border bg-[#FFF8EE] px-4 py-6">
				<BrandLogo {...args} theme="light" className="h-12" />
			</div>
			<div className="rounded-2xl border bg-[#252A30] px-4 py-6">
				<BrandLogo {...args} theme="dark" className="h-12" />
			</div>
		</div>
	),
	parameters: {
		docs: {
			description: {
				story:
					"横向字标，供 README、Web 品牌位与 docs-site 深浅色场景共同使用。",
			},
		},
	},
};

export const MarkOnly: Story = {
	args: {
		variant: "mark",
	},
	render: (args) => (
		<div className="bg-background grid gap-4 p-6 md:grid-cols-2">
			<div className="flex h-28 items-center justify-center rounded-3xl bg-[#FFF8EE] p-4">
				<BrandLogo {...args} className="h-full" />
			</div>
			<div className="flex h-28 items-center justify-center rounded-3xl bg-[#252A30] p-4">
				<img
					alt="OctoRill favicon preview"
					className="h-[72px] w-[72px]"
					src="/brand/favicon.svg"
				/>
			</div>
		</div>
	),
	parameters: {
		docs: {
			description: {
				story:
					"透明主标与圆角 favicon / app icon 预览，供窄导航位与浏览器标签页使用。",
			},
		},
	},
};

export const SurfaceGalleryStory: Story = {
	name: "Surface gallery",
	render: () => <SurfaceGallery />,
	parameters: {
		docs: {
			description: {
				story:
					"把独立资产、Landing badge、Dashboard header 与 Admin header 摆到同一个稳定 review 面里，用于本地视觉验收与后续截图复用。",
			},
		},
	},
};
