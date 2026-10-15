import {
	createContext,
	type ReactNode,
	useCallback,
	useContext,
	useMemo,
	useState,
} from "react";

// Device-local on purpose: whether the Queue Drawer pushes the page aside is
// a per-window habit, so it lives in this client's storage, not the server's
// user preferences.
const QUEUE_PINNED_KEY = "earthly-audio-queue-pinned";

function readQueuePinned(): boolean {
	if (typeof window === "undefined") return true;
	try {
		return window.localStorage.getItem(QUEUE_PINNED_KEY) !== "false";
	} catch {
		return true;
	}
}

function writeQueuePinned(pinned: boolean) {
	try {
		window.localStorage.setItem(QUEUE_PINNED_KEY, String(pinned));
	} catch {
		// Storage can be unavailable (private windows); the choice then lasts
		// only for this session.
	}
}

type QueuePinValue = { pinned: boolean; togglePinned: () => void };

const QueuePinContext = createContext<QueuePinValue | null>(null);

/**
 * Pinned (the default), the open Queue Drawer takes its own column and the
 * page makes room for it; unpinned, it floats over the page.
 */
export function QueuePinProvider({ children }: { children: ReactNode }) {
	const [pinned, setPinned] = useState(readQueuePinned);
	const togglePinned = useCallback(() => {
		setPinned((current) => {
			writeQueuePinned(!current);
			return !current;
		});
	}, []);
	const value = useMemo(
		() => ({ pinned, togglePinned }),
		[pinned, togglePinned],
	);
	return (
		<QueuePinContext.Provider value={value}>
			{children}
		</QueuePinContext.Provider>
	);
}

/** Null outside the App Shell, where the Queue has no drawer to pin. */
export function useQueuePin() {
	return useContext(QueuePinContext);
}
