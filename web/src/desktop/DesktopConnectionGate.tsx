import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import {
	type ConnectionCheck,
	getServerConnection,
	initializeMediaProxy,
	isDesktopClient,
	saveServerConnection,
	testServerConnection,
} from "./bridge";

type DesktopConnectionGateProps = {
	children: ReactNode;
};

export function DesktopConnectionGate({
	children,
}: DesktopConnectionGateProps) {
	const isDesktop = isDesktopClient();
	const [isLoading, setIsLoading] = useState(isDesktop);
	const [isConnected, setIsConnected] = useState(!isDesktop);
	const [origin, setOrigin] = useState("http://127.0.0.1:8090");
	const [verified, setVerified] = useState<ConnectionCheck | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [isTesting, setIsTesting] = useState(false);
	const [isSaving, setIsSaving] = useState(false);

	useEffect(() => {
		if (!isDesktop) return;
		let isCancelled = false;

		async function verifySavedConnection() {
			try {
				const saved = await getServerConnection();
				if (!saved) return;
				if (!isCancelled) setOrigin(saved.origin);
				await testServerConnection(saved.origin);
				await initializeMediaProxy();
				if (!isCancelled) setIsConnected(true);
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

	if (!isDesktop || isConnected) return children;
	if (isLoading) {
		return (
			<div className="grid h-full place-items-center">
				Checking Music Server…
			</div>
		);
	}

	async function handleTest(event: FormEvent) {
		event.preventDefault();
		setIsTesting(true);
		setError(null);
		setVerified(null);
		try {
			const check = await testServerConnection(origin);
			setOrigin(check.origin);
			setVerified(check);
		} catch (connectionError) {
			setError(connectionErrorMessage(connectionError));
		} finally {
			setIsTesting(false);
		}
	}

	async function handleSave() {
		setIsSaving(true);
		setError(null);
		try {
			await saveServerConnection(origin);
			await initializeMediaProxy();
			setIsConnected(true);
		} catch (connectionError) {
			setVerified(null);
			setError(connectionErrorMessage(connectionError));
		} finally {
			setIsSaving(false);
		}
	}

	return (
		<main className="grid h-full place-items-center p-6">
			<section className="flex w-full max-w-lg flex-col gap-6 rounded-lg border bg-card p-8">
				<div className="flex flex-col gap-2">
					<p className="text-sm font-medium text-primary">
						Earthly Audio Desktop
					</p>
					<h1 className="text-2xl font-semibold">
						Connect to your Music Server
					</h1>
					<p className="text-sm text-muted-foreground">
						Development builds accept only a server bound to this machine.
					</p>
				</div>

				<form className="flex flex-col gap-4" onSubmit={handleTest}>
					<div className="flex flex-col gap-2">
						<Label htmlFor="music-server-origin">Music Server URL</Label>
						<Input
							id="music-server-origin"
							type="url"
							value={origin}
							onChange={(event) => {
								setOrigin(event.target.value);
								setVerified(null);
							}}
							placeholder="http://127.0.0.1:8090"
							required
						/>
					</div>
					{error ? (
						<p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
							{error}
						</p>
					) : null}
					{verified ? (
						<p className="rounded-md border bg-muted p-3 text-sm">
							Connected to Music Server {verified.version}. Required
							capabilities available.
						</p>
					) : null}
					<div className="flex justify-end gap-3">
						<Button
							type="submit"
							variant="outline"
							disabled={isTesting || isSaving}
						>
							{isTesting ? "Testing…" : "Test connection"}
						</Button>
						<Button
							type="button"
							disabled={!verified || verified.origin !== origin || isSaving}
							onClick={handleSave}
						>
							{isSaving ? "Saving…" : "Save and connect"}
						</Button>
					</div>
				</form>
			</section>
		</main>
	);
}

function connectionErrorMessage(error: unknown): string {
	if (typeof error === "object" && error !== null && "message" in error) {
		const message = (error as { message?: unknown }).message;
		if (typeof message === "string") return message;
	}
	if (error instanceof Error) return error.message;
	return "Music Server connection failed. Check the URL and try again.";
}
