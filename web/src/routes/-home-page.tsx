import { PageHeader, PageShell } from "#/components/page-layout";

export function HomePage() {
	return (
		<PageShell
			testId="home-page-shell"
			contentTestId="home-page-content"
			header={<PageHeader title="Home" />}
		>
			<div className="rounded-md border border-dashed border-border p-6 text-caption text-sm">
				Home layout will be designed later. Add music from the Tracks page with
				Import Music.
			</div>
		</PageShell>
	);
}
