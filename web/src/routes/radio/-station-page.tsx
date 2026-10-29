import type { RadioStationPatch } from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { Play, Radio, Star } from "lucide-react";
import { useState } from "react";
import { DetailPageShell } from "#/components/page-layout";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { apiClient } from "#/lib/api";
import { radioQueryKeys } from "#/lib/radio-query-keys";
import {
	type RadioStationForm,
	radioStationFormSchema,
} from "#/lib/radio-station-form";
import { isGone } from "#/lib/query-persistence";

// Route components live outside the route file: with automatic code
// splitting, a component defined inside the route module can be rendered
// from its split chunk before that chunk finished evaluating after an HMR
// update in WebKit, which surfaces as "_s is not a function".
export function RadioStationDetailPage() {
	const { stationId } = useParams({ from: "/radio/$stationId" });
	return <RadioStationDetailContent stationId={stationId} />;
}

export function RadioStationDetailContent({
	stationId,
}: {
	stationId: string;
}) {
	const queryClient = useQueryClient();
	const { playRadioStation, currentRadioStation } = usePlayback();
	const station = useQuery({
		queryKey: radioQueryKeys.detail(stationId),
		queryFn: () => apiClient.getRadioStation(stationId),
	});
	const [form, setForm] = useState<RadioStationForm>({
		name: "",
		streamUrl: "",
		homepageUrl: "",
		faviconUrl: "",
		country: "",
		language: "",
		tags: "",
		codec: "",
		bitrate: "",
	});

	// Each fetched version of the station resets the form to its values.
	const [formSource, setFormSource] = useState<typeof station.data>(undefined);
	if (station.data && station.data !== formSource) {
		setFormSource(station.data);
		setForm({
			name: station.data.name,
			streamUrl: station.data.streamUrl,
			homepageUrl: station.data.homepageUrl ?? "",
			faviconUrl: station.data.faviconUrl ?? "",
			country: station.data.country ?? "",
			language: station.data.language ?? "",
			tags: station.data.tags.join(", "),
			codec: station.data.codec ?? "",
			bitrate: station.data.bitrate ? String(station.data.bitrate) : "",
		});
	}

	const updateStation = useMutation({
		mutationFn: (patch: RadioStationPatch) =>
			apiClient.patchRadioStation(stationId, patch),
		onSuccess: async () => {
			await Promise.all([
				queryClient.invalidateQueries({
					queryKey: radioQueryKeys.detail(stationId),
				}),
				queryClient.invalidateQueries({
					queryKey: radioQueryKeys.stations,
				}),
			]);
		},
	});

	if (station.isLoading) {
		return (
			<div className="p-6 text-foreground text-sm">Loading station...</div>
		);
	}

	if (!station.data || isGone(station.error)) {
		return (
			<div className="p-6">
				<Link
					to="/radio"
					className="mb-6 inline-block text-foreground text-sm hover:text-heading"
				>
					Back to radio
				</Link>
				<p className="text-destructive text-sm">Radio station not found</p>
			</div>
		);
	}

	const data = station.data;
	const parsedForm = radioStationFormSchema.safeParse(form);
	const isActive = currentRadioStation?.id === data.id;

	return (
		<DetailPageShell
			testId="radio-station-detail-content"
			className="flex flex-col gap-6"
		>
			<Link to="/radio" className="text-foreground text-sm hover:text-heading">
				Back to radio
			</Link>

			<header className="flex flex-col gap-4 rounded-md border border-border bg-card/35 p-4">
				<div className="flex min-w-0 items-center gap-4">
					<StationArtwork faviconUrl={data.faviconUrl} name={data.name} />
					<div className="min-w-0 flex-1">
						<h1 className="truncate font-semibold text-2xl text-heading">
							{data.name}
						</h1>
						<p className="truncate text-caption text-sm">{data.streamUrl}</p>
					</div>
					<Badge variant="outline">{data.source}</Badge>
				</div>
				{data.lastNowPlaying?.raw ? (
					<p className="text-foreground text-sm">{data.lastNowPlaying.raw}</p>
				) : null}
				<div className="flex flex-wrap items-center gap-2">
					<Button onClick={() => void playRadioStation(data)}>
						<Play className="size-4" />
						{isActive ? "Playing" : "Play"}
					</Button>
					<Button
						variant="outline"
						disabled={updateStation.isPending}
						onClick={() =>
							updateStation.mutate({ isFavorite: !data.isFavorite })
						}
					>
						<Star
							className={
								data.isFavorite ? "size-4 fill-current text-heading" : "size-4"
							}
						/>
						{data.isFavorite ? "Remove favorite" : "Add favorite"}
					</Button>
				</div>
			</header>

			<section className="grid gap-4 rounded-md border border-border bg-card/25 p-4 md:grid-cols-2">
				<h2 className="font-semibold text-heading text-xl md:col-span-2">
					Metadata
				</h2>
				<MetadataField
					id="radio-detail-name"
					label="Name"
					value={form.name}
					onChange={(name) => setForm((current) => ({ ...current, name }))}
				/>
				<MetadataField
					id="radio-detail-stream-url"
					label="Stream URL"
					type="url"
					value={form.streamUrl}
					onChange={(streamUrl) =>
						setForm((current) => ({ ...current, streamUrl }))
					}
				/>
				<MetadataField
					id="radio-detail-homepage-url"
					label="Homepage URL"
					type="url"
					value={form.homepageUrl}
					onChange={(homepageUrl) =>
						setForm((current) => ({ ...current, homepageUrl }))
					}
				/>
				<MetadataField
					id="radio-detail-favicon-url"
					label="Favicon URL"
					type="url"
					value={form.faviconUrl}
					onChange={(faviconUrl) =>
						setForm((current) => ({ ...current, faviconUrl }))
					}
				/>
				<MetadataField
					id="radio-detail-country"
					label="Country"
					value={form.country}
					onChange={(country) =>
						setForm((current) => ({ ...current, country }))
					}
				/>
				<MetadataField
					id="radio-detail-language"
					label="Language"
					value={form.language}
					onChange={(language) =>
						setForm((current) => ({ ...current, language }))
					}
				/>
				<MetadataField
					id="radio-detail-tags"
					label="Tags"
					value={form.tags}
					onChange={(tags) => setForm((current) => ({ ...current, tags }))}
				/>
				<MetadataField
					id="radio-detail-codec"
					label="Codec"
					value={form.codec}
					onChange={(codec) => setForm((current) => ({ ...current, codec }))}
				/>
				<MetadataField
					id="radio-detail-bitrate"
					label="Bitrate"
					type="number"
					value={form.bitrate}
					onChange={(bitrate) =>
						setForm((current) => ({ ...current, bitrate }))
					}
				/>
				<div className="flex items-end">
					<Button
						disabled={!parsedForm.success || updateStation.isPending}
						onClick={() => {
							if (parsedForm.success) updateStation.mutate(parsedForm.data);
						}}
					>
						Save metadata
					</Button>
				</div>
				{updateStation.isError ? (
					<p className="text-destructive text-sm md:col-span-2">
						Failed to save station metadata
					</p>
				) : null}
			</section>
		</DetailPageShell>
	);
}

function MetadataField({
	id,
	label,
	value,
	onChange,
	type = "text",
}: {
	id: string;
	label: string;
	value: string;
	onChange: (value: string) => void;
	type?: "number" | "text" | "url";
}) {
	return (
		<label className="flex min-w-0 flex-col gap-2" htmlFor={id}>
			<span className="font-medium text-caption text-xs">{label}</span>
			<Input
				id={id}
				type={type}
				value={value}
				onChange={(event) => onChange(event.target.value)}
			/>
		</label>
	);
}

function StationArtwork({
	faviconUrl,
	name,
}: {
	faviconUrl?: string;
	name: string;
}) {
	return faviconUrl ? (
		<img
			alt={name}
			className="size-16 shrink-0 rounded-md border border-border bg-muted object-cover"
			src={faviconUrl}
		/>
	) : (
		<div
			aria-hidden
			className="flex size-16 shrink-0 items-center justify-center rounded-md border border-border bg-muted text-caption"
		>
			<Radio className="size-6" />
		</div>
	);
}
