import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { RowMenuButton } from "./RowMenuButton";

describe("RowMenuButton", () => {
	afterEach(cleanup);

	it("opens the row's context menu instead of acting on the row", () => {
		const onContextMenu = vi.fn();
		const onClick = vi.fn();
		const onKeyDown = vi.fn();
		render(
			// biome-ignore lint/a11y/noStaticElementInteractions: stands in for a row.
			<div
				role="presentation"
				onContextMenu={onContextMenu}
				onClick={onClick}
				onKeyDown={onKeyDown}
			>
				<RowMenuButton label="Actions for Song" />
			</div>,
		);
		const button = screen.getByRole("button", { name: "Actions for Song" });

		fireEvent.keyDown(button, { key: "Enter" });
		fireEvent.click(button);

		expect(onContextMenu).toHaveBeenCalledOnce();
		expect(onClick).not.toHaveBeenCalled();
		expect(onKeyDown).not.toHaveBeenCalled();
	});

	it("reaches a trigger that sits right before it", () => {
		const onContextMenu = vi.fn();
		render(
			<div>
				<button type="button" onContextMenu={onContextMenu}>
					Card
				</button>
				<RowMenuButton label="Card actions" afterTrigger />
			</div>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Card actions" }));

		expect(onContextMenu).toHaveBeenCalledOnce();
	});
});
