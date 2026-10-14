import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
	cleanup,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { ImportSessionProvider } from "./import-session-provider";

vi.mock("@repo/ui", () => ({ clearTrackWaveformCache: vi.fn() }));
vi.mock("#/routes/library/tracks/-import-music-dialog", () => ({
	ImportMusicDialog: ({
		onCommitted,
	}: {
		onCommitted: () => Promise<void>;
	}) => (
		<button type="button" onClick={() => void onCommitted()}>
			Commit import
		</button>
	),
}));
afterEach(cleanup);

it("invalidates all Library Search categories after a committed import", async () => {
	const client = new QueryClient();
	const key = ["library", "search", "new artist"];
	client.setQueryData(key, { artists: [] });
	render(
		<QueryClientProvider client={client}>
			<ImportSessionProvider>
				<p>Library</p>
			</ImportSessionProvider>
		</QueryClientProvider>,
	);
	fireEvent.click(screen.getByRole("button", { name: "Commit import" }));
	await waitFor(() =>
		expect(client.getQueryState(key)?.isInvalidated).toBe(true),
	);
});
