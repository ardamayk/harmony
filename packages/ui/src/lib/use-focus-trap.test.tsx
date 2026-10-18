import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { useFocusTrap } from "./use-focus-trap";

function Dialog({ active = true }: { active?: boolean }) {
	const ref = useRef<HTMLDivElement>(null);
	useFocusTrap(ref, active);
	return (
		<div ref={ref} role="dialog" aria-label="Trap" tabIndex={-1}>
			<button type="button">First</button>
			<button type="button">Second</button>
		</div>
	);
}

describe("useFocusTrap", () => {
	afterEach(cleanup);

	it("moves focus in, cycles at both edges and restores focus on unmount", () => {
		const outside = document.createElement("button");
		outside.textContent = "Outside";
		document.body.appendChild(outside);
		outside.focus();

		const view = render(<Dialog />);
		const first = screen.getByRole("button", { name: "First" });
		const second = screen.getByRole("button", { name: "Second" });
		expect(document.activeElement).toBe(first);

		second.focus();
		fireEvent.keyDown(document, { key: "Tab" });
		expect(document.activeElement).toBe(first);

		fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
		expect(document.activeElement).toBe(second);

		view.unmount();
		expect(document.activeElement).toBe(outside);
		outside.remove();
	});

	it("does nothing while inactive", () => {
		render(<Dialog active={false} />);
		expect(document.activeElement).toBe(document.body);
	});
});
