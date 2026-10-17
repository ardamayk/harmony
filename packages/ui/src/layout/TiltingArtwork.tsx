import { type PointerEvent, useRef } from "react";
import { AlbumArt } from "./AlbumArt";

const MAX_TILT_DEGREES = 12;
const RESTING_TRANSFORM = "rotateX(0deg) rotateY(0deg)";

export function TiltingArtwork({
	coverUrl,
	title,
}: {
	coverUrl: string | null;
	title: string;
}) {
	const artworkRef = useRef<HTMLDivElement>(null);
	function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
		if (
			event.pointerType === "touch" ||
			window.matchMedia("(prefers-reduced-motion: reduce)").matches
		)
			return;
		const bounds = event.currentTarget.getBoundingClientRect();
		if (!artworkRef.current || !bounds.width || !bounds.height) return;
		const horizontal = Math.max(
			-1,
			Math.min(1, ((event.clientX - bounds.left) / bounds.width) * 2 - 1),
		);
		const vertical = Math.max(
			-1,
			Math.min(1, ((event.clientY - bounds.top) / bounds.height) * 2 - 1),
		);
		artworkRef.current.style.transform = `rotateX(${-vertical * MAX_TILT_DEGREES}deg) rotateY(${horizontal * MAX_TILT_DEGREES}deg)`;
	}
	function resetTilt() {
		if (artworkRef.current)
			artworkRef.current.style.transform = RESTING_TRANSFORM;
	}
	return (
		<div
			className="w-[min(100%,65vh)] shrink-0 p-4 [perspective:1200px]"
			onPointerMove={handlePointerMove}
			onPointerLeave={resetTilt}
			onPointerCancel={resetTilt}
		>
			<div
				ref={artworkRef}
				data-testid="tilting-artwork"
				className="aspect-square w-full border border-border shadow-xl transition-transform duration-200 ease-out motion-reduce:!transform-none motion-reduce:transition-none"
			>
				<AlbumArt
					key={coverUrl}
					coverUrl={coverUrl}
					title={title}
					className="size-full"
				/>
			</div>
		</div>
	);
}
