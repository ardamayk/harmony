import { useHiddenFromGuest } from "@repo/ui/guest-access";
import { PageHeader, PageShell } from "#/components/page-layout";

export function HomePage() {
	const hideImport = useHiddenFromGuest("managed-import");
	return (
		<PageShell
			testId="home-page-shell"
			contentTestId="home-page-content"
			header={<PageHeader title="Home" />}
		>
			<div className="rounded-md border border-dashed border-border p-6 text-caption text-sm">
				Home layout will be designed later.
				{hideImport ? null : " Add music from Settings with Import Music."}
			</div>
		</PageShell>
	);
}
