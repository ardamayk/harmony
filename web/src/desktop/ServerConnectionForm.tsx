import { type FormEvent, useState } from "react";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import {
	type ConnectionCheck,
	type ServerHeaders,
	saveServerConnection,
	testServerConnection,
} from "./bridge";

type HeaderRow = { name: string; value: string };

const EMPTY_HEADER: HeaderRow = { name: "", value: "" };
const MAX_HEADER_ROWS = 8;

/** The rows the user filled in, or undefined to leave saved headers alone. */
function serverHeaders(rows: HeaderRow[]): ServerHeaders | undefined {
	const filled = rows.filter((row) => row.name.trim() !== "");
	if (filled.length === 0) return undefined;
	return Object.fromEntries(filled.map((row) => [row.name.trim(), row.value]));
}

export function connectionErrorMessage(error: unknown): string {
	if (typeof error === "object" && error !== null && "message" in error) {
		const message = (error as { message?: unknown }).message;
		if (typeof message === "string") return message;
	}
	if (error instanceof Error) return error.message;
	return "Music Server connection failed. Check the URL and try again.";
}

/** Tests a Music Server, then saves it; first run and Settings share this. */
export function ServerConnectionForm({
	initialOrigin,
	initialError = null,
	savedHeaderNames = [],
	onConnected,
}: {
	initialOrigin: string;
	initialError?: string | null;
	/** The saved server's headers, which empty rows keep. */
	savedHeaderNames?: string[];
	onConnected: () => Promise<void> | void;
}) {
	const [origin, setOrigin] = useState(initialOrigin);
	const [headerRows, setHeaderRows] = useState([EMPTY_HEADER]);
	const [removeSaved, setRemoveSaved] = useState(false);
	const [verified, setVerified] = useState<ConnectionCheck | null>(null);
	const [error, setError] = useState(initialError);
	const [isTesting, setIsTesting] = useState(false);
	const [isSaving, setIsSaving] = useState(false);
	const headers = serverHeaders(headerRows) ?? (removeSaved ? {} : undefined);

	async function handleTest(event: FormEvent) {
		event.preventDefault();
		setIsTesting(true);
		setError(null);
		setVerified(null);
		try {
			const check = await testServerConnection(origin, headers);
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
			await saveServerConnection(origin, headers);
			await onConnected();
		} catch (connectionError) {
			setVerified(null);
			setError(connectionErrorMessage(connectionError));
		} finally {
			setIsSaving(false);
		}
	}

	return (
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
					placeholder="https://music.example.com"
					required
				/>
			</div>
			{origin.startsWith("https://") ? (
				<HeaderFields
					rows={headerRows}
					savedNames={removeSaved ? [] : savedHeaderNames}
					onRemoveSaved={() => {
						setRemoveSaved(true);
						setVerified(null);
					}}
					onChange={(rows) => {
						setHeaderRows(rows);
						setVerified(null);
					}}
				/>
			) : null}
			{error ? (
				<p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
					{error}
				</p>
			) : null}
			{verified ? (
				<p className="rounded-md border bg-muted p-3 text-sm">
					Connected to Music Server {verified.version}. Required capabilities
					available.
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
	);
}

/** For a proxy in front of the server; the values stay on this device. */
function HeaderFields({
	rows,
	savedNames,
	onRemoveSaved,
	onChange,
}: {
	rows: HeaderRow[];
	savedNames: string[];
	onRemoveSaved: () => void;
	onChange: (rows: HeaderRow[]) => void;
}) {
	const update = (index: number, change: Partial<HeaderRow>) =>
		onChange(
			rows.map((row, rowIndex) =>
				rowIndex === index ? { ...row, ...change } : row,
			),
		);
	return (
		<div className="flex flex-col gap-3">
			<p className="text-sm text-muted-foreground">
				Custom headers, if a proxy in front of your server asks for them. They
				are stored encrypted on this device.
			</p>
			{savedNames.length > 0 ? (
				<div className="flex items-center justify-between gap-3">
					<p className="min-w-0 text-sm text-muted-foreground">
						Saved for this server: {savedNames.join(", ")}. Headers entered
						below replace them.
					</p>
					<Button type="button" variant="outline" onClick={onRemoveSaved}>
						Remove saved headers
					</Button>
				</div>
			) : null}
			{rows.map((row, index) => (
				<div key={index} className="grid grid-cols-2 gap-3">
					<div className="flex flex-col gap-2">
						<Label htmlFor={`server-header-name-${index}`}>Header name</Label>
						<Input
							id={`server-header-name-${index}`}
							value={row.name}
							autoComplete="off"
							spellCheck={false}
							onChange={(event) => update(index, { name: event.target.value })}
						/>
					</div>
					<div className="flex flex-col gap-2">
						<Label htmlFor={`server-header-value-${index}`}>Header value</Label>
						<Input
							id={`server-header-value-${index}`}
							type="password"
							value={row.value}
							autoComplete="off"
							onChange={(event) => update(index, { value: event.target.value })}
						/>
					</div>
				</div>
			))}
			<div>
				<Button
					type="button"
					variant="outline"
					disabled={rows.length >= MAX_HEADER_ROWS}
					onClick={() => onChange([...rows, EMPTY_HEADER])}
				>
					Add header
				</Button>
			</div>
		</div>
	);
}
