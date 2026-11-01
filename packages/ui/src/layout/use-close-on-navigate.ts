import { useRouter } from "@tanstack/react-router";
import { useEffect } from "react";

/** A full-screen view closes when one of its links opens a page beneath it. */
export function useCloseOnNavigate(onClose: () => void) {
	const router = useRouter({ warn: false });
	useEffect(
		() =>
			router?.subscribe("onBeforeNavigate", (event) => {
				if (event.pathChanged) onClose();
			}),
		[router, onClose],
	);
}
