import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { ServerDependenciesSection } from "./-server-dependencies";

const health = {
	status: "ok" as const,
	version: "0.1.0",
	capabilities: ["api.v1"],
	dependencies: [
		{ name: "ffmpeg", required: true, available: true, version: "7.1.1" },
		{ name: "ffprobe", required: true, available: true, version: "7.1.1" },
	],
};

describe("ServerDependenciesSection", () => {
	afterEach(() => {
		cleanup();
	});

	it("lists every Server Dependency with its version and requirement", () => {
		render(<ServerDependenciesSection health={health} desktopMpv={null} />);

		const ffmpeg = screen.getByRole("row", { name: /ffmpeg/ });
		expect(ffmpeg.textContent).toContain("7.1.1");
		expect(ffmpeg.textContent).toContain("Required");
		expect(ffmpeg.textContent).toContain("Installed");

		expect(screen.queryByText(/Recording Identification/)).toBeNull();
	});

	it("omits the mpv row on the Web Client and shows it on the Desktop Client", () => {
		const { unmount } = render(
			<ServerDependenciesSection health={health} desktopMpv={null} />,
		);
		expect(screen.queryByRole("row", { name: /mpv/ })).toBeNull();
		unmount();

		render(
			<ServerDependenciesSection
				health={health}
				desktopMpv={{
					available: true,
					pinned: true,
					pinnedVersion: "0.40.0",
					version: "0.40.0",
				}}
			/>,
		);
		const mpv = screen.getByRole("row", { name: /mpv/ });
		expect(mpv.textContent).toContain("0.40.0");
		expect(mpv.textContent).toContain("Desktop Client");
	});

	it("shows an installed but unpinned mpv as installed with the pin detail", () => {
		render(
			<ServerDependenciesSection
				health={health}
				desktopMpv={{
					available: true,
					pinned: false,
					pinnedVersion: "0.40.0",
					version: "0.39.0",
					detail: "expected pinned mpv 0.40.0",
				}}
			/>,
		);

		const mpv = screen.getByRole("row", { name: /mpv/ });
		expect(mpv.textContent).toContain("Installed");
		expect(mpv.textContent).toContain("expected pinned mpv 0.40.0");
	});

	it("tolerates a Music Server that predates the dependency report", () => {
		render(
			<ServerDependenciesSection
				health={
					{
						status: "ok",
						version: "0.0.9",
						capabilities: ["api.v1"],
					} as unknown as typeof health
				}
				desktopMpv={null}
			/>,
		);

		expect(screen.queryByRole("row", { name: /ffmpeg/ })).toBeNull();
		expect(screen.queryByText(/Recording Identification/)).toBeNull();
	});

	it("says who answers for the External Source only when it is offered", () => {
		const { unmount } = render(
			<ServerDependenciesSection health={health} desktopMpv={null} />,
		);
		expect(screen.queryByText(/External Source/)).toBeNull();
		unmount();

		render(
			<ServerDependenciesSection
				health={{
					...health,
					capabilities: ["api.v1", "external-source.v1"],
					dependencies: [
						...health.dependencies,
						{
							name: "yt-dlp",
							required: false,
							available: true,
							version: "2026.10.26",
						},
					],
				}}
				desktopMpv={null}
			/>,
		);
		expect(screen.getByText(/configured by the operator/).textContent).toMatch(
			/responsible for the source and for following the site’s terms/,
		);
		const ytDlp = screen.getByRole("row", { name: /yt-dlp/ });
		expect(ytDlp.textContent).toContain("Optional");
	});

	it("renders a loading state without health data", () => {
		render(<ServerDependenciesSection health={undefined} desktopMpv={null} />);

		expect(screen.getByText(/Loading server dependencies/)).toBeTruthy();
	});
});
