import {
	getTrackArtistCredits,
	goToArtistCreditsSearch,
	type QueueRowMenuProps,
	toast,
	usePlayback,
	usePlaylistLibrary,
} from "@repo/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Menu } from "@base-ui/react/menu";
import { MoreHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSub,
	DropdownMenuSubContent,
	DropdownMenuSubTrigger,
	DropdownMenuTrigger,
} from "#/components/ui/dropdown-menu";
import {
	invalidatePlaylistCache,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";

export function QueueTrackMenu({ item, children }: QueueRowMenuProps) {
	const [isContextOpen, setIsContextOpen] = useState(false);
	const [isDropdownOpen, setIsDropdownOpen] = useState(false);
	// The dropdown trigger sits inside the context menu's trigger row; a handle
	// keeps the dropdown its own menu instead of a child of the context menu.
	const [dropdown] = useState(() => Menu.createHandle());
	const trigger = (
		<DropdownMenuTrigger
			handle={dropdown}
			data-queue-menu-trigger
			aria-label={`Queue actions for ${item.track.title}`}
			className="absolute top-1/2 right-2 inline-flex size-8 -translate-y-1/2 items-center justify-center rounded-sm text-caption opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-player-pill hover:text-heading focus-visible:outline focus-visible:outline-2 data-popup-open:opacity-100"
			onClick={(event) => event.stopPropagation()}
			onPointerDown={(event) => event.stopPropagation()}
			onKeyDown={(event) => event.stopPropagation()}
		>
			<MoreHorizontal className="size-4" aria-hidden />
		</DropdownMenuTrigger>
	);

	return (
		<>
			<ContextMenu onOpenChange={setIsContextOpen}>
				<ContextMenuTrigger
					render={children(trigger)}
					onKeyDownCapture={(event) => {
						if (
							event.key !== "ContextMenu" &&
							!(event.shiftKey && event.key === "F10")
						)
							return;
						event.preventDefault();
						event.stopPropagation();
						const bounds = event.currentTarget.getBoundingClientRect();
						event.currentTarget.dispatchEvent(
							new MouseEvent("contextmenu", {
								bubbles: true,
								cancelable: true,
								clientX: bounds.left,
								clientY: bounds.bottom,
							}),
						);
					}}
				/>
				<ContextMenuContent
					onClick={(event) => event.stopPropagation()}
					onKeyDown={(event) => event.stopPropagation()}
				>
					{isContextOpen && <QueueMenuActions item={item} />}
				</ContextMenuContent>
			</ContextMenu>
			<DropdownMenu
				handle={dropdown}
				open={isDropdownOpen}
				onOpenChange={setIsDropdownOpen}
			>
				<DropdownMenuContent
					align="end"
					onClick={(event) => event.stopPropagation()}
					onKeyDown={(event) => event.stopPropagation()}
				>
					{isDropdownOpen && <QueueMenuActions item={item} />}
				</DropdownMenuContent>
			</DropdownMenu>
		</>
	);
}

// Base UI's ContextMenu reuses the Menu parts, so one set of items serves both.
function QueueMenuActions({ item }: { item: QueueRowMenuProps["item"] }) {
	const { playNext, removeFromQueue } = usePlayback();
	const { listPlaylists, addPlaylistTrack } = usePlaylistLibrary();
	const queryClient = useQueryClient();
	const navigate = useNavigate();
	const [isPlaylistOpen, setIsPlaylistOpen] = useState(false);
	const playlists = useQuery({
		queryKey: playlistQueryKeys.list,
		queryFn: listPlaylists,
		enabled: isPlaylistOpen,
	});
	useEffect(() => {
		if (!playlists.error) return;
		console.warn("Failed to load playlists for queue track", {
			trackId: item.track.id,
			error: playlists.error,
		});
		toast.error("Failed to load playlists");
	}, [playlists.error, item.track.id]);

	const runAction = (message: string, action: () => Promise<unknown>) => {
		void (async () => {
			try {
				await action();
			} catch (error) {
				console.warn(message, {
					queueItemId: item.id,
					trackId: item.track.id,
					error,
				});
				toast.error(message);
			}
		})();
	};
	const artists = getTrackArtistCredits(item.track);
	const goToArtist = (artistId: string) =>
		runAction("Failed to open artist", () =>
			Promise.resolve(goToArtistCreditsSearch(navigate, artistId)),
		);
	return (
		<>
			<DropdownMenuItem
				onClick={() =>
					runAction("Failed to play track next", () => playNext(item.track.id))
				}
			>
				Play next
			</DropdownMenuItem>
			<DropdownMenuSub open={isPlaylistOpen} onOpenChange={setIsPlaylistOpen}>
				<DropdownMenuSubTrigger>Add to playlist</DropdownMenuSubTrigger>
				<DropdownMenuSubContent
					onClick={(event) => event.stopPropagation()}
					onKeyDown={(event) => event.stopPropagation()}
				>
					{playlists.isPending ? (
						<DropdownMenuItem disabled>Loading playlists…</DropdownMenuItem>
					) : playlists.isError ? (
						<DropdownMenuItem
							closeOnClick={false}
							onClick={() => void playlists.refetch()}
						>
							Retry loading playlists
						</DropdownMenuItem>
					) : playlists.data?.items.length === 0 ? (
						<DropdownMenuItem disabled>No playlists</DropdownMenuItem>
					) : (
						playlists.data?.items.map((playlist) => (
							<DropdownMenuItem
								key={playlist.id}
								onClick={() =>
									runAction("Failed to add track to playlist", async () => {
										await addPlaylistTrack(playlist.id, item.track.id);
										await invalidatePlaylistCache(queryClient, playlist.id);
										toast.success(`Added to ${playlist.name}`);
									})
								}
							>
								{playlist.name}
							</DropdownMenuItem>
						))
					)}
				</DropdownMenuSubContent>
			</DropdownMenuSub>
			<DropdownMenuItem
				onClick={() =>
					runAction("Failed to open album", () =>
						navigate({
							to: "/library/$albumId",
							params: { albumId: item.track.albumId },
						}),
					)
				}
			>
				Go to album
			</DropdownMenuItem>
			{artists.length > 1 ? (
				<DropdownMenuSub>
					<DropdownMenuSubTrigger>Go to artist</DropdownMenuSubTrigger>
					<DropdownMenuSubContent
						onClick={(event) => event.stopPropagation()}
						onKeyDown={(event) => event.stopPropagation()}
					>
						{artists.map((artist) => (
							<DropdownMenuItem
								key={artist.id}
								onClick={() => goToArtist(artist.id)}
							>
								{artist.name}
							</DropdownMenuItem>
						))}
					</DropdownMenuSubContent>
				</DropdownMenuSub>
			) : (
				<DropdownMenuItem
					disabled={!artists.length}
					onClick={() => goToArtist(artists[0].id)}
				>
					Go to artist
				</DropdownMenuItem>
			)}
			<DropdownMenuItem
				variant="destructive"
				onClick={() =>
					runAction("Failed to remove track from queue", () =>
						removeFromQueue(item.id),
					)
				}
			>
				Remove from queue
			</DropdownMenuItem>
		</>
	);
}
