import { autoUpdater } from "electron-updater";

const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/**
 * Checks the GitHub Releases named in the packaged app-update.yml. A newer
 * Desktop Client downloads in the background, the system notification says so,
 * and it installs when Harmony quits. A check that fails waits for the next one.
 */
export function startUpdateChecks() {
	const check = () => {
		autoUpdater.checkForUpdatesAndNotify().catch((error: unknown) => {
			process.stderr.write(`[shell] update check failed: ${String(error)}\n`);
		});
	};
	// An `error` event with no listener would crash the main process.
	autoUpdater.on("error", (error) => {
		process.stderr.write(`[shell] update failed: ${String(error)}\n`);
	});
	check();
	setInterval(check, CHECK_INTERVAL_MS);
}
