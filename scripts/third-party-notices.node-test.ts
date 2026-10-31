import assert from "node:assert/strict";
import { test } from "node:test";
import { renderNotices } from "./third-party-notices.ts";

test("a shared licence text is printed once, naming everyone under it", () => {
	const notices = renderNotices([
		{ name: "b 1.0.0", license: "MIT", texts: ["MIT text"] },
		{ name: "a 1.0.0", license: "MIT", texts: ["MIT text"] },
		{ name: "c 2.0.0", license: "ISC", texts: [] },
	]);
	assert.equal(notices.split("MIT text").length, 2);
	assert.match(notices, /a 1\.0\.0, b 1\.0\.0\n\nMIT text/);
	assert.match(notices, /c 2\.0\.0: ISC/);
});
