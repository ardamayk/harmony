// electron-builder's afterPack hook: the icon must be in Harmony.exe before an
// installer wraps it. See scripts/desktop-windows/icon.ts for why electron-builder does not set it.

import { join } from "node:path";
import { setExecutableIcon } from "../../scripts/desktop-windows/icon.ts";

export default async function afterPack({ appOutDir, electronPlatformName }) {
	if (electronPlatformName !== "win32") return;
	setExecutableIcon(
		join(appOutDir, "Harmony.exe"),
		join(import.meta.dirname, "../resources/icon.ico"),
	);
}
