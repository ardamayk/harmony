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
import { ChevronRight, MoreHorizontal } from "lucide-react";
import { ContextMenu as ContextMenuPrimitive, DropdownMenu } from "radix-ui";
import { useEffect, useState } from "react";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import {
	invalidatePlaylistCache,
	playlistQueryKeys,
} from "#/lib/playlist-query-cache";

const MENU_CONTENT_CLASS =
	"z-50 min-w-40 overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg";
const MENU_ITEM_CLASS =
	"relative flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-2 text-sm outline-hidden focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50";

export function QueueTrackMenu({ item, children }: QueueRowMenuProps) {
	const [isContextOpen, setIsContextOpen] = useState(false);
	const [isDropdownOpen, setIsDropdownOpen] = useState(false);
	const trigger = (
		<DropdownMenu.Trigger asChild>
			<button
				type="button"
				data-queue-menu-trigger
				aria-label={`Queue actions for ${item.track.title}`}
				className="absolute top-1/2 right-2 inline-flex size-8 -translate-y-1/2 items-center justify-center rounded-sm text-caption opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-player-pill hover:text-heading focus-visible:outline focus-visible:outline-2 data-[state=open]:opacity-100"
				onClick={(event) => event.stopPropagation()}
				onPointerDown={(event) => event.stopPropagation()}
				onKeyDown={(event) => event.stopPropagation()}
			>
				<MoreHorizontal className="size-4" aria-hidden />
			</button>
		</DropdownMenu.Trigger>
	);

	return (
		<ContextMenu onOpenChange={setIsContextOpen}>
			<DropdownMenu.Root open={isDropdownOpen} onOpenChange={setIsDropdownOpen}>
				<ContextMenuTrigger
					asChild
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
				>
					{children(trigger)}
				</ContextMenuTrigger>
				<ContextMenuContent
					onClick={(event) => event.stopPropagation()}
					onKeyDown={(event) => event.stopPropagation()}
				>
					{isContextOpen && <QueueMenuActions item={item} mode="context" />}
				</ContextMenuContent>
				<DropdownMenu.Portal>
					<DropdownMenu.Content
						align="end"
						sideOffset={4}
						className={MENU_CONTENT_CLASS}
						onClick={(event) => event.stopPropagation()}
						onKeyDown={(event) => event.stopPropagation()}
					>
						{isDropdownOpen && <QueueMenuActions item={item} mode="dropdown" />}
					</DropdownMenu.Content>
				</DropdownMenu.Portal>
			</DropdownMenu.Root>
		</ContextMenu>
	);
}

function QueueMenuActions({
	item,
	mode,
}: {
	item: QueueRowMenuProps["item"];
	mode: "context" | "dropdown";
}) {
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
	const Menu = mode === "context" ? ContextMenuPrimitive : DropdownMenu;
	const Item = mode === "context" ? ContextMenuItem : DropdownMenu.Item;
	return (
		<>
			<Item
				className={MENU_ITEM_CLASS}
				onSelect={() =>
					runAction("Failed to play track next", () => playNext(item.track.id))
				}
			>
				Play next
			</Item>
			<Menu.Sub open={isPlaylistOpen} onOpenChange={setIsPlaylistOpen}>
				<Menu.SubTrigger className={MENU_ITEM_CLASS}>
					Add to playlist{" "}
					<ChevronRight className="ml-auto size-4" aria-hidden />
				</Menu.SubTrigger>
				<Menu.Portal>
					<Menu.SubContent
						className={MENU_CONTENT_CLASS}
						onClick={(event) => event.stopPropagation()}
						onKeyDown={(event) => event.stopPropagation()}
					>
						{playlists.isPending ? (
							<Item className={MENU_ITEM_CLASS} disabled>
								Loading playlists…
							</Item>
						) : playlists.isError ? (
							<Item
								className={MENU_ITEM_CLASS}
								onSelect={(event) => {
									event.preventDefault();
									void playlists.refetch();
								}}
							>
								Retry loading playlists
							</Item>
						) : playlists.data?.items.length === 0 ? (
							<Item className={MENU_ITEM_CLASS} disabled>
								No playlists
							</Item>
						) : (
							playlists.data?.items.map((playlist) => (
								<Item
									key={playlist.id}
									className={MENU_ITEM_CLASS}
									onSelect={() =>
										runAction("Failed to add track to playlist", async () => {
											await addPlaylistTrack(playlist.id, item.track.id);
											await invalidatePlaylistCache(queryClient, playlist.id);
											toast.success(`Added to ${playlist.name}`);
										})
									}
								>
									{playlist.name}
								</Item>
							))
						)}
					</Menu.SubContent>
				</Menu.Portal>
			</Menu.Sub>
			<Item
				className={MENU_ITEM_CLASS}
				onSelect={() =>
					runAction("Failed to open album", () =>
						navigate({
							to: "/library/$albumId",
							params: { albumId: item.track.albumId },
						}),
					)
				}
			>
				Go to album
			</Item>
			{artists.length > 1 ? (
				<Menu.Sub>
					<Menu.SubTrigger className={MENU_ITEM_CLASS}>
						Go to artist <ChevronRight className="ml-auto size-4" aria-hidden />
					</Menu.SubTrigger>
					<Menu.Portal>
						<Menu.SubContent
							className={MENU_CONTENT_CLASS}
							onClick={(event) => event.stopPropagation()}
							onKeyDown={(event) => event.stopPropagation()}
						>
							{artists.map((artist) => (
								<Item
									key={artist.id}
									className={MENU_ITEM_CLASS}
									onSelect={() => goToArtist(artist.id)}
								>
									{artist.name}
								</Item>
							))}
						</Menu.SubContent>
					</Menu.Portal>
				</Menu.Sub>
			) : (
				<Item
					className={MENU_ITEM_CLASS}
					disabled={!artists.length}
					onSelect={() => goToArtist(artists[0].id)}
				>
					Go to artist
				</Item>
			)}
			<Item
				className={`${MENU_ITEM_CLASS} text-destructive`}
				onSelect={() =>
					runAction("Failed to remove track from queue", () =>
						removeFromQueue(item.id),
					)
				}
			>
				Remove from queue
			</Item>
		</>
	);
}
