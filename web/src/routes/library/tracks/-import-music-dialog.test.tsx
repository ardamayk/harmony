import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { ImportFileList } from "./-import-music-dialog";
import type { ImportFileEntry } from "./-managed-import-workflow";

function entry(
	name: string,
	overrides: Partial<ImportFileEntry> = {},
): ImportFileEntry {
	return {
		key: name,
		file: new File([], name),
		progress: 0,
		state: "unresolved",
		selected: false,
		hasSelectionOverride: false,
		...overrides,
	};
}

function accepted(name: string, title: string): ImportFileEntry {
	return entry(name, {
		state: "accepted",
		phase: "ready",
		progress: 100,
		selected: true,
		preview: {
			file: {
				albumKey: "album-1",
				album: "Album",
				albumArtists: ["Artist"],
				artists: ["Artist"],
				title,
			},
		} as ImportFileEntry["preview"],
	});
}

function renderList(entries: ImportFileEntry[], isProcessing: boolean) {
	render(
		<ImportFileList
			entries={entries}
			isBusy={isProcessing}
			isConfirming={false}
			isProcessing={isProcessing}
			renderAlbumHeader={(group) => <h4>{group.album}</h4>}
			onSelectionChange={() => {}}
			onDuplicateDecisionChange={() => {}}
		/>,
	);
}

const rowLabels = () =>
	screen
		.getAllByRole("article")
		.map((row) => row.querySelector("[title]")?.getAttribute("title"));

afterEach(cleanup);

describe("import file list", () => {
	it("keeps rows in place without groups while files are processed", () => {
		renderList(
			[
				entry("1.flac", { phase: "validating" }),
				accepted("2.flac", "Second"),
				entry("3.flac", { phase: "queued" }),
			],
			true,
		);
		expect(rowLabels()).toEqual(["1.flac", "Second", "3.flac"]);
		expect(screen.queryByRole("heading", { level: 4 })).toBeNull();
		expect(screen.queryByRole("checkbox")).toBeNull();
	});

	it("marks every phase with its own icon and label", () => {
		renderList(
			[
				entry("1.flac", { phase: "validating" }),
				accepted("2.flac", "Second"),
				entry("3.flac", { phase: "queued" }),
			],
			true,
		);
		const [validating, ready, queued] = screen.getAllByRole("article");
		expect(validating.textContent).toContain("Validating…");
		const spinner = validating.querySelector("svg")?.getAttribute("class");
		expect(spinner).toContain("motion-safe:animate-spin");
		expect(spinner).toContain("text-warning");
		expect(ready.textContent).toContain("Accepted");
		expect(ready.querySelector("svg")?.getAttribute("class")).toContain(
			"text-success",
		);
		expect(queued.textContent).toContain("Queued");
		expect(queued.querySelector("svg")?.getAttribute("class")).toContain(
			"text-caption",
		);
	});

	it("groups settled files and hands selection to checkboxes", () => {
		renderList(
			[
				accepted("1.flac", "First"),
				accepted("2.flac", "Second"),
				entry("3.flac", { state: "rejected", phase: "failed" }),
			],
			false,
		);
		expect(
			screen.getAllByRole("heading", { level: 4 }).map((h) => h.textContent),
		).toEqual(["Album", "Selected files"]);
		expect(screen.getAllByRole("checkbox")).toHaveLength(2);
	});
});
