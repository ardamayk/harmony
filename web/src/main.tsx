import { QueryClient } from "@tanstack/react-query";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { getServerConnection, isDesktopClient } from "./desktop/bridge";
import { hasDesktopBridge } from "./desktop/desktop-api";
import { getApiBaseUrl } from "./env";
import { suppressEscapeFocusRing } from "./lib/escape-focus-ring";
import {
	createQueryPersister,
	QUERY_CACHE_BUSTER,
	QUERY_CACHE_MAX_AGE_MS,
	shouldPersistQuery,
} from "./lib/query-persistence";
import { routeTree } from "./routeTree.gen";
import "./styles.css";

const ReactQueryDevtools = lazy(async () => {
	const module = await import("@tanstack/react-query-devtools");
	return { default: module.ReactQueryDevtools };
});

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			refetchOnWindowFocus: true,
			// The Music Server is often on this machine or the home network:
			// "no internet" must not pause requests that would reach it.
			networkMode: "always",
		},
		mutations: { networkMode: "always" },
	},
});
// Only queries still in memory are saved, so the saved scopes stay as long
// as their saves; search results keep the default five minutes.
for (const scope of [["library"], ["playlists"], ["playlist"], ["radio"]]) {
	queryClient.setQueryDefaults(scope, { gcTime: QUERY_CACHE_MAX_AGE_MS });
}
queryClient.setQueryDefaults(["library", "search"], { gcTime: 5 * 60 * 1000 });

const queryPersister = createQueryPersister(async () =>
	isDesktopClient()
		? ((await getServerConnection())?.origin ?? "")
		: getApiBaseUrl() || window.location.origin,
);

const router = createRouter({
	routeTree,
	context: { queryClient },
	defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
	interface Register {
		router: typeof router;
	}
}

const rootElement = document.getElementById("root");
if (!rootElement) {
	throw new Error("Root element not found");
}

suppressEscapeFocusRing();

const shouldRenderQueryDevtools = import.meta.env.DEV && !hasDesktopBridge();

createRoot(rootElement).render(
	<StrictMode>
		<PersistQueryClientProvider
			client={queryClient}
			persistOptions={{
				persister: queryPersister,
				maxAge: QUERY_CACHE_MAX_AGE_MS,
				buster: QUERY_CACHE_BUSTER,
				dehydrateOptions: { shouldDehydrateQuery: shouldPersistQuery },
			}}
		>
			<div id="app">
				<RouterProvider router={router} />
			</div>
			{shouldRenderQueryDevtools ? (
				<Suspense fallback={null}>
					<ReactQueryDevtools initialIsOpen={false} />
				</Suspense>
			) : null}
		</PersistQueryClientProvider>
	</StrictMode>,
);
