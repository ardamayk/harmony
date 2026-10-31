import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "#/components/ui/button";
import { getServerConnection } from "#/desktop/bridge";
import { ServerConnectionForm } from "#/desktop/ServerConnectionForm";

/** Which Music Server this Desktop Client talks to, and how; changeable here. */
export function DesktopConnectionSection() {
	const [isEditing, setIsEditing] = useState(false);
	const connection = useQuery({
		queryKey: ["desktop", "server-connection"],
		queryFn: getServerConnection,
	});
	const saved = connection.data;
	return (
		<section
			aria-labelledby="desktop-connection-heading"
			className="mb-8 flex flex-col gap-4"
		>
			<h2 id="desktop-connection-heading" className="font-medium text-sm">
				Music Server
			</h2>
			{saved ? (
				<dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
					<dt className="text-caption">URL</dt>
					<dd className="min-w-0 truncate">{saved.origin}</dd>
					<dt className="text-caption">Custom headers</dt>
					<dd className="min-w-0 truncate">
						{saved.headerNames.length > 0
							? saved.headerNames.join(", ")
							: "None"}
					</dd>
				</dl>
			) : null}
			{isEditing ? (
				<ServerConnectionForm
					initialOrigin={saved?.origin ?? ""}
					savedHeaderNames={saved?.headerNames}
					// Everything on screen came from the previous server.
					onConnected={() => window.location.reload()}
				/>
			) : (
				<Button
					type="button"
					variant="outline"
					className="self-start"
					onClick={() => setIsEditing(true)}
				>
					Change Music Server
				</Button>
			)}
		</section>
	);
}
