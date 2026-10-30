import assert from "node:assert/strict";
import { test } from "node:test";
import { isWsl, selectDesktopTarget } from "./desktop-target.ts";

const linuxKernel = "Linux version 6.8.0-45-generic (buildd@lcy02-amd64-115)";
const wslKernel = "Linux version 6.6.87.2-microsoft-standard-WSL2";

test("WSL is recognised by WSL_DISTRO_NAME or the kernel name", () => {
	assert.equal(isWsl({ WSL_DISTRO_NAME: "Ubuntu" }, linuxKernel), true);
	assert.equal(isWsl({}, wslKernel), true);
	assert.equal(isWsl({}, "Linux version 4.4.0-19041-Microsoft"), true);
	assert.equal(isWsl({}, linuxKernel), false);
	assert.equal(isWsl({ WSL_DISTRO_NAME: "" }, undefined), false);
});

test("without an override, WSL builds Windows and native Linux builds Linux", () => {
	assert.equal(
		selectDesktopTarget({ WSL_DISTRO_NAME: "Ubuntu" }, wslKernel),
		"windows",
	);
	assert.equal(selectDesktopTarget({}, wslKernel), "windows");
	assert.equal(selectDesktopTarget({}, linuxKernel), "linux");
	assert.equal(selectDesktopTarget({ CI: "true" }, undefined), "linux");
	assert.equal(
		selectDesktopTarget({ EARTHLY_DESKTOP_TARGET: "" }, wslKernel),
		"windows",
	);
});

test("EARTHLY_DESKTOP_TARGET overrides the choice", () => {
	assert.equal(
		selectDesktopTarget(
			{ EARTHLY_DESKTOP_TARGET: "linux", WSL_DISTRO_NAME: "Ubuntu" },
			wslKernel,
		),
		"linux",
	);
	assert.equal(
		selectDesktopTarget({ EARTHLY_DESKTOP_TARGET: "linux" }, linuxKernel),
		"linux",
	);
	assert.equal(
		selectDesktopTarget({ EARTHLY_DESKTOP_TARGET: "windows" }, wslKernel),
		"windows",
	);
});

test("windows outside WSL is refused", () => {
	assert.throws(
		() =>
			selectDesktopTarget({ EARTHLY_DESKTOP_TARGET: "windows" }, linuxKernel),
		/works only in WSL/,
	);
});

test("other values are refused, case included", () => {
	for (const value of ["Windows", "mac", "win", " linux"]) {
		assert.throws(
			() => selectDesktopTarget({ EARTHLY_DESKTOP_TARGET: value }, wslKernel),
			/must be linux or windows/,
		);
	}
});
