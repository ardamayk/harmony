import { PLAYER_BAR_HEIGHT_PX } from "@repo/ui";
import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import {
	getServerConnection,
	initializeMediaProxy,
	isDesktopClient,
	testServerConnection,
} from "./bridge";
import {
	connectionErrorMessage,
	ServerConnectionForm,
} from "./ServerConnectionForm";

type DesktopConnectionGateProps = {
	children: ReactNode;
};

/** How often an unreachable saved server is tried again. */
const OFFLINE_RETRY_MS = 15_000;

export function DesktopConnectionGate({
	children,
}: DesktopConnectionGateProps) {
	const isDesktop = isDesktopClient();
	const [isLoading, setIsLoading] = useState(isDesktop);
	const [isConnected, setIsConnected] = useState(!isDesktop);
	const [origin, setOrigin] = useState("http://127.0.0.1:8090");
	const [error, setError] = useState<string | null>(null);
	// The saved server did not answer: the app opens on its saved lists.
	const [offlineOrigin, setOfflineOrigin] = useState<string | null>(null);
	const [isRetrying, setIsRetrying] = useState(false);
	const queryClient = useQueryClient();

	useEffect(() => {
		if (!isDesktop) return;
		let isCancelled = false;

		async function verifySavedConnection() {
			try {
				const saved = await getServerConnection();
				if (!saved) return;
				if (!isCancelled) setOrigin(saved.origin);
				let reachable = true;
				try {
					await testServerConnection(saved.origin);
				} catch (connectionError) {
					if (!isUnreachable(connectionError)) throw connectionError;
					reachable = false;
				}
				await initializeMediaProxy();
				if (isCancelled) return;
				if (!reachable) setOfflineOrigin(saved.origin);
				setIsConnected(true);
			} catch (connectionError) {
				if (!isCancelled) setError(connectionErrorMessage(connectionError));
			} finally {
				if (!isCancelled) setIsLoading(false);
			}
		}

		void verifySavedConnection();
		return () => {
			isCancelled = true;
		};
	}, [isDesktop]);

	const retryOffline = useCallback(async () => {
		if (!offlineOrigin) return;
		setIsRetrying(true);
		try {
			await testServerConnection(offlineOrigin);
			setOfflineOrigin(null);
			// Everything shown so far came from the saved lists.
			void queryClient.invalidateQueries();
		} catch {
			// Still unreachable; the notice stays.
		} finally {
			setIsRetrying(false);
		}
	}, [offlineOrigin, queryClient]);

	useEffect(() => {
		if (!offlineOrigin) return;
		const timer = setInterval(() => void retryOffline(), OFFLINE_RETRY_MS);
		return () => clearInterval(timer);
	}, [offlineOrigin, retryOffline]);

	if (!isDesktop || isConnected) {
		return (
			<>
				{children}
				{offlineOrigin ? (
					<OfflineNotice
						origin={offlineOrigin}
						isRetrying={isRetrying}
						onRetry={() => void retryOffline()}
					/>
				) : null}
			</>
		);
	}
	if (isLoading) {
		return (
			<div className="grid h-full place-items-center">
				Checking Music Server…
			</div>
		);
	}

	return (
		<main className="grid h-full place-items-center p-6">
			<section className="flex w-full max-w-lg flex-col gap-6 rounded-lg border bg-card p-8">
				<div className="flex flex-col gap-2">
					<p className="text-sm font-medium text-primary">Harmony Desktop</p>
					<h1 className="text-2xl font-semibold">
						Connect to your Music Server
					</h1>
					<p className="text-sm text-muted-foreground">
						Enter an https address, or a loopback address on this machine.
					</p>
				</div>
				<ServerConnectionForm
					initialOrigin={origin}
					initialError={error}
					onConnected={async () => {
						await initializeMediaProxy();
						setIsConnected(true);
					}}
				/>
			</section>
		</main>
	);
}

/** Sits above the Player Bar, like the playback error notice. */
function OfflineNotice({
	origin,
	isRetrying,
	onRetry,
}: {
	origin: string;
	isRetrying: boolean;
	onRetry: () => void;
}) {
	return (
		<div
			role="status"
			className="fixed left-1/2 z-50 flex max-w-lg -translate-x-1/2 items-center gap-3 rounded-md border border-border bg-popover px-3 py-2 text-sm shadow-lg"
			style={{ bottom: `calc(${PLAYER_BAR_HEIGHT_PX}px + 0.75rem)` }}
		>
			<div className="min-w-0">
				<p className="text-foreground">Can't reach the Music Server</p>
				<p className="text-caption text-xs">
					Showing saved lists from {origin}. Nothing can play until it's back.
				</p>
			</div>
			<button
				type="button"
				className="shrink-0 rounded-sm border border-border px-2 py-1 text-xs hover:bg-muted disabled:opacity-50"
				disabled={isRetrying}
				onClick={onRetry}
			>
				{isRetrying ? "Trying…" : "Try again"}
			</button>
		</div>
	);
}

/** The Desktop core's code for a server that does not answer at all. */
function isUnreachable(error: unknown): boolean {
	return (
		typeof error === "object" &&
		error !== null &&
		(error as { code?: unknown }).code === "unreachable"
	);
}
