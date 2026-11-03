import { ApiError } from "@repo/api-client";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { ListenTogetherNotice } from "#/components/listen-together-gate";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { getServerConnection, isDesktopClient } from "#/desktop/bridge";
import { ServerConnectionForm } from "#/desktop/ServerConnectionForm";
import { apiClient } from "#/lib/api";
import { markSignedIn, useSignedOut } from "#/lib/sign-in";
import { stopSharedPlaybackEngine } from "#/playback/shared-playback-engine";

/**
 * Shows the sign-in screen in place of the app once the Music Server answers
 * that this client must sign in (ADR 0042); without sign-in configured it
 * never does, and the app renders as before.
 */
export function SignInGate({ children }: { children: ReactNode }) {
	const signedOut = useSignedOut();
	// Nothing the old session started may keep playing.
	useEffect(() => {
		if (signedOut) stopSharedPlaybackEngine();
	}, [signedOut]);
	if (!signedOut) return children;
	if (isDesktopClient()) return <DesktopSignIn />;
	return (
		<ListenTogetherNotice
			title="Sign in"
			description="This Music Server asks for your username and password."
		>
			<SignInForm />
		</ListenTogetherNotice>
	);
}

export function signInErrorMessage(error: unknown): string {
	if (!(error instanceof ApiError)) {
		return "Couldn't reach the Music Server. Check that it is running, then try again.";
	}
	if (error.status === 401) {
		return "Wrong username or password. Check them and try again.";
	}
	if (error.status === 429) {
		const seconds = error.retryAfterSeconds ?? 60;
		const [count, unit] =
			seconds < 60 ? [seconds, "second"] : [Math.ceil(seconds / 60), "minute"];
		return `Too many failed attempts. Try again in ${count} ${unit}${count === 1 ? "" : "s"}.`;
	}
	return error.message;
}

function SignInForm() {
	const queryClient = useQueryClient();
	const [error, setError] = useState<string | null>(null);
	const [isSigningIn, setIsSigningIn] = useState(false);

	function enterApp() {
		markSignedIn();
		void queryClient.invalidateQueries();
	}

	async function handleSubmit(event: FormEvent<HTMLFormElement>) {
		event.preventDefault();
		const form = new FormData(event.currentTarget);
		setIsSigningIn(true);
		setError(null);
		try {
			await apiClient.login({
				username: form.get("username") as string,
				password: form.get("password") as string,
			});
			enterApp();
		} catch (loginError) {
			// Sign-in was turned off since the last answer.
			if (loginError instanceof ApiError && loginError.status === 404) {
				enterApp();
				return;
			}
			setError(signInErrorMessage(loginError));
			setIsSigningIn(false);
		}
	}

	return (
		<form
			className="flex w-full max-w-sm flex-col gap-4 text-left"
			onSubmit={handleSubmit}
		>
			<div className="flex flex-col gap-2">
				<Label htmlFor="sign-in-username">Username</Label>
				<Input
					id="sign-in-username"
					name="username"
					autoComplete="username"
					autoCapitalize="none"
					spellCheck={false}
					required
				/>
			</div>
			<div className="flex flex-col gap-2">
				<Label htmlFor="sign-in-password">Password</Label>
				<Input
					id="sign-in-password"
					name="password"
					type="password"
					autoComplete="current-password"
					required
				/>
			</div>
			{error ? (
				<p
					role="alert"
					className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
				>
					{error}
				</p>
			) : null}
			<Button type="submit" disabled={isSigningIn}>
				{isSigningIn ? "Signing in…" : "Sign in"}
			</Button>
		</form>
	);
}

/**
 * The core keeps no cookies, so the Desktop Client signs in with Basic
 * credentials saved among the server's encrypted headers (ADR 0038).
 */
function DesktopSignIn() {
	const connection = useQuery({
		queryKey: ["desktop", "server-connection"],
		queryFn: getServerConnection,
	});
	return (
		<ListenTogetherNotice
			title="Sign in"
			description="This Music Server asks for your username and password. They are stored encrypted on this device."
		>
			{connection.data ? (
				<div className="w-full max-w-lg text-left">
					<ServerConnectionForm
						initialOrigin={connection.data.origin}
						savedHeaderNames={connection.data.headerNames}
						asksForSignIn
						// The core sends the new header from now on; mount everything again.
						onConnected={() => window.location.reload()}
					/>
				</div>
			) : null}
		</ListenTogetherNotice>
	);
}
