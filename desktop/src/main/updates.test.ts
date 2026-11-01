import { EventEmitter } from "node:events";
import type { DesktopUpdateStatus } from "@repo/desktop-bridge";
import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from "vite-plus/test";
import {
	type UpdateControllerOptions,
	UpdateController,
	type Updater,
	updateTrayItem,
} from "./updates";

const NEWER = "0.1.18";
const available = (version = NEWER) => ({
	isUpdateAvailable: true,
	updateInfo: { version },
});
const upToDate = { isUpdateAvailable: false, updateInfo: { version: "0.1.0" } };

function fakeUpdater() {
	return Object.assign(new EventEmitter(), {
		autoDownload: true,
		autoInstallOnAppQuit: true,
		checkForUpdates: vi.fn(),
		downloadUpdate: vi.fn(),
		quitAndInstall: vi.fn(),
	});
}

function setup(options: Partial<UpdateControllerOptions> = {}) {
	const updater = fakeUpdater();
	const changes: DesktopUpdateStatus[] = [];
	const stopForInstall = vi.fn(async () => {});
	const onInstallFailed = vi.fn();
	const controller = new UpdateController({
		currentVersion: "0.1.17",
		updater: updater as unknown as Updater,
		onChange: (status) => changes.push(status),
		stopForInstall,
		onInstallFailed,
		...options,
	});
	return { updater, changes, stopForInstall, onInstallFailed, controller };
}

async function reachAvailable(context: ReturnType<typeof setup>) {
	context.updater.checkForUpdates.mockResolvedValue(available());
	await context.controller.check();
}

async function reachDownloaded(context: ReturnType<typeof setup>) {
	await reachAvailable(context);
	context.updater.downloadUpdate.mockResolvedValue([]);
	await context.controller.download();
}

describe("UpdateController", () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.spyOn(process.stderr, "write").mockReturnValue(true);
	});
	afterEach(() => {
		vi.useRealTimers();
		vi.restoreAllMocks();
	});

	it("turns off downloading and installing on its own", async () => {
		const { updater, controller } = setup();
		updater.checkForUpdates.mockResolvedValue(available());

		controller.start();
		await vi.advanceTimersByTimeAsync(0);

		expect(updater.autoDownload).toBe(false);
		expect(updater.autoInstallOnAppQuit).toBe(false);
		expect(updater.downloadUpdate).not.toHaveBeenCalled();
		expect(updater.quitAndInstall).not.toHaveBeenCalled();
	});

	it("checks at start and every six hours, and stops at the offer", async () => {
		const { updater, controller } = setup();
		updater.checkForUpdates.mockResolvedValue(available());

		controller.start();
		await vi.advanceTimersByTimeAsync(0);
		expect(updater.checkForUpdates).toHaveBeenCalledOnce();
		expect(controller.status()).toEqual({
			currentVersion: "0.1.17",
			status: "available",
			version: NEWER,
		});

		await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
		expect(updater.checkForUpdates).toHaveBeenCalledTimes(2);
		expect(updater.downloadUpdate).not.toHaveBeenCalled();
	});

	it("keeps an automatic check that fails out of the state", async () => {
		const { updater, changes, controller } = setup();
		updater.checkForUpdates.mockRejectedValue(new Error("offline"));

		controller.start();
		await vi.advanceTimersByTimeAsync(0);

		expect(controller.status().status).toBe("idle");
		expect(changes).toEqual([]);
	});

	it("shows a check the user started, then its answer", async () => {
		const { updater, changes, controller } = setup();
		updater.checkForUpdates.mockResolvedValue(upToDate);

		await controller.check();

		expect(changes.map((change) => change.status)).toEqual([
			"checking",
			"up-to-date",
		]);
	});

	it("shows a failed check the user started, and retries it", async () => {
		const { updater, controller } = setup();
		updater.checkForUpdates.mockRejectedValueOnce(new Error("offline"));

		await controller.check();
		expect(controller.status()).toMatchObject({
			status: "failed",
			action: "check",
		});

		updater.checkForUpdates.mockResolvedValue(available());
		await controller.check();
		expect(controller.status().status).toBe("available");
	});

	it("reports the download in whole percents", async () => {
		const context = setup();
		const { updater, changes, controller } = context;
		controller.start();
		await reachAvailable(context);
		const finished = Promise.withResolvers<string[]>();
		updater.downloadUpdate.mockReturnValue(finished.promise);

		const download = controller.download();
		updater.emit("download-progress", { percent: 41.2 });
		updater.emit("download-progress", { percent: 41.9 });
		updater.emit("download-progress", { percent: 42.1 });
		finished.resolve([]);
		await download;

		expect(changes.slice(-4)).toEqual([
			expect.objectContaining({ status: "downloading", percent: 0 }),
			expect.objectContaining({ status: "downloading", percent: 41 }),
			expect.objectContaining({ status: "downloading", percent: 42 }),
			expect.objectContaining({ status: "downloaded", version: NEWER }),
		]);
		expect(updater.quitAndInstall).not.toHaveBeenCalled();
	});

	it("never shows a differential download's early 100", async () => {
		const context = setup();
		const { updater, changes, controller } = context;
		controller.start();
		await reachAvailable(context);
		const finished = Promise.withResolvers<string[]>();
		updater.downloadUpdate.mockReturnValue(finished.promise);

		const download = controller.download();
		updater.emit("download-progress", { percent: 100 });
		updater.emit("download-progress", { percent: 19 });
		updater.emit("download-progress", { percent: 100 });
		finished.resolve([]);
		await download;

		expect(changes.slice(-3).map((change) => change.status)).toEqual([
			"downloading",
			"downloading",
			"downloaded",
		]);
		expect(controller.status()).not.toHaveProperty("percent");
		expect(changes.at(-2)).toMatchObject({ percent: 19 });
	});

	it("downloads nothing until an update is on offer", async () => {
		const { updater, controller } = setup();

		await controller.download();

		expect(updater.downloadUpdate).not.toHaveBeenCalled();
	});

	it("keeps the version through a failed download and retries it", async () => {
		const context = setup();
		const { updater, controller } = context;
		await reachAvailable(context);
		updater.downloadUpdate.mockRejectedValueOnce(new Error("reset"));

		await controller.download();
		expect(controller.status()).toMatchObject({
			status: "failed",
			action: "download",
			version: NEWER,
		});

		updater.downloadUpdate.mockResolvedValue([]);
		await controller.download();
		expect(controller.status().status).toBe("downloaded");
	});

	it("keeps the downloaded update through later checks", async () => {
		const context = setup();
		const { updater, controller } = context;
		controller.start();
		await reachDownloaded(context);
		updater.checkForUpdates.mockClear();
		updater.checkForUpdates.mockResolvedValue(available("0.1.19"));

		await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
		await controller.check();

		expect(updater.checkForUpdates).not.toHaveBeenCalled();
		expect(controller.status()).toMatchObject({
			status: "downloaded",
			version: NEWER,
		});
	});

	it("lets a download outrank an automatic check still answering", async () => {
		const { updater, controller } = setup();
		const check = Promise.withResolvers<unknown>();
		updater.checkForUpdates.mockResolvedValueOnce(available());
		updater.checkForUpdates.mockReturnValueOnce(check.promise);
		updater.downloadUpdate.mockReturnValue(new Promise(() => {}));
		controller.start();
		await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);

		void controller.download();
		check.resolve(available("0.1.19"));
		await vi.advanceTimersByTimeAsync(0);

		expect(controller.status()).toMatchObject({
			status: "downloading",
			version: NEWER,
		});
	});

	it("restarts only after the click, once the core has stopped", async () => {
		const context = setup();
		const { updater, stopForInstall, controller } = context;
		await reachDownloaded(context);
		expect(updater.quitAndInstall).not.toHaveBeenCalled();
		const order: string[] = [];
		stopForInstall.mockImplementation(async () => {
			order.push("stop");
		});
		updater.quitAndInstall.mockImplementation(() => order.push("install"));

		await controller.install();

		expect(order).toEqual(["stop", "install"]);
		expect(updater.quitAndInstall).toHaveBeenCalledWith(true, true);
		expect(controller.status().status).toBe("installing");
	});

	it("refuses to install before a download finished", async () => {
		const context = setup();
		const { updater, stopForInstall, controller } = context;
		await reachAvailable(context);

		await controller.install();

		expect(stopForInstall).not.toHaveBeenCalled();
		expect(updater.quitAndInstall).not.toHaveBeenCalled();
	});

	it("installs once however often the button is pressed", async () => {
		const context = setup();
		const { updater, controller } = context;
		await reachDownloaded(context);

		await Promise.all([controller.install(), controller.install()]);

		expect(updater.quitAndInstall).toHaveBeenCalledOnce();
	});

	it("reports an installer that did not start", async () => {
		const context = setup();
		const { updater, onInstallFailed, controller } = context;
		controller.start();
		await reachDownloaded(context);
		updater.emit("error", new Error("unrelated"));
		expect(onInstallFailed).not.toHaveBeenCalled();

		await controller.install();
		updater.emit("error", new Error("no installer"));

		expect(onInstallFailed).toHaveBeenCalledOnce();
	});

	it("offers nothing where the client cannot update", async () => {
		const { controller } = setup({ updater: null });

		controller.start();
		await controller.check();
		await controller.download();
		await controller.install();

		expect(controller.status()).toEqual({
			currentVersion: "0.1.17",
			status: "unavailable",
		});
	});
});

describe("updateTrayItem", () => {
	it("mirrors only the pending action", () => {
		expect(updateTrayItem({ status: "idle" })).toBeNull();
		expect(updateTrayItem({ status: "checking" })).toBeNull();
		expect(updateTrayItem({ status: "up-to-date" })).toBeNull();
		expect(updateTrayItem({ status: "unavailable" })).toBeNull();
		expect(updateTrayItem({ status: "failed", action: "check" })).toBeNull();
		expect(updateTrayItem({ status: "available", version: NEWER })).toEqual({
			label: `Download update ${NEWER}`,
			action: "download",
		});
		expect(
			updateTrayItem({ status: "downloading", version: NEWER, percent: 42 }),
		).toEqual({ label: "Downloading update 42%" });
		expect(updateTrayItem({ status: "downloaded", version: NEWER })).toEqual({
			label: "Restart to update",
			action: "install",
		});
		expect(updateTrayItem({ status: "installing", version: NEWER })).toEqual({
			label: "Restarting to update",
		});
		expect(
			updateTrayItem({ status: "failed", action: "download", version: NEWER }),
		).toEqual({ label: "Retry update download", action: "download" });
	});
});
