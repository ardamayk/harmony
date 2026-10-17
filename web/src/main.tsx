import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { hasDesktopBridge } from "./desktop/desktop-api";
import { suppressEscapeFocusRing } from "./lib/escape-focus-ring";
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
		},
	},
});

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
		<QueryClientProvider client={queryClient}>
			<div id="app">
				<RouterProvider router={router} />
			</div>
			{shouldRenderQueryDevtools ? (
				<Suspense fallback={null}>
					<ReactQueryDevtools initialIsOpen={false} />
				</Suspense>
			) : null}
		</QueryClientProvider>
	</StrictMode>,
);
