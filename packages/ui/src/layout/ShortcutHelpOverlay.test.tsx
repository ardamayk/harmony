import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { ShortcutHelpOverlay } from "./ShortcutHelpOverlay";

describe("ShortcutHelpOverlay", () => {
	afterEach(cleanup);

	it("lists the shortcuts in a dialog and closes on Escape", () => {
		const onClose = vi.fn();
		render(<ShortcutHelpOverlay onClose={onClose} />);

		const dialog = screen.getByRole("dialog", { name: "Keyboard shortcuts" });
		expect(dialog.textContent).toContain("Play or pause");
		expect(dialog.textContent).toContain("Seek 5 seconds");

		fireEvent.keyDown(document, { key: "Escape" });
		expect(onClose).toHaveBeenCalledTimes(1);

		fireEvent.click(screen.getByRole("button", { name: "Close" }));
		expect(onClose).toHaveBeenCalledTimes(2);
	});

	it("reflects custom seek steps", () => {
		render(
			<ShortcutHelpOverlay
				options={{ seekStepSeconds: 10, seekStepLargeSeconds: 60 }}
				onClose={() => undefined}
			/>,
		);
		expect(screen.getByText("Seek 10 seconds")).toBeTruthy();
		expect(screen.getByText("Seek 60 seconds")).toBeTruthy();
	});
});
