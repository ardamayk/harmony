import {
	AUTH_SESSION_CAPABILITY,
	type HealthResponse,
	hasServerCapability,
} from "@repo/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "#/components/ui/button";
import { apiClient } from "#/lib/api";
import { markSignedOut } from "#/lib/sign-in";

/**
 * Sign out of a Music Server with sign-in (ADR 0042). The Desktop Client
 * signs in with saved headers instead, which Music Server above changes.
 */
export function AccountSection({
	health,
}: {
	health: HealthResponse | undefined;
}) {
	const queryClient = useQueryClient();
	const [isSigningOut, setIsSigningOut] = useState(false);
	const [error, setError] = useState<string | null>(null);
	if (!hasServerCapability(health, AUTH_SESSION_CAPABILITY)) return null;

	async function signOut() {
		setIsSigningOut(true);
		setError(null);
		try {
			await apiClient.logout();
			markSignedOut();
			// The library's saved lists must not outlast the session.
			queryClient.clear();
		} catch {
			setError(
				"Couldn't sign out. Check that the Music Server is running, then try again.",
			);
			setIsSigningOut(false);
		}
	}

	return (
		<section
			aria-labelledby="account-heading"
			className="mb-8 flex flex-col gap-4"
		>
			<h2 id="account-heading" className="font-medium text-sm">
				Account
			</h2>
			<Button
				type="button"
				variant="outline"
				className="self-start"
				disabled={isSigningOut}
				onClick={() => void signOut()}
			>
				Sign out
			</Button>
			{error ? (
				<p role="alert" className="text-destructive text-sm">
					{error}
				</p>
			) : null}
		</section>
	);
}
