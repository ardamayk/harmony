import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

const env = createEnv({
	clientPrefix: "VITE_",
	client: {
		VITE_API_URL: z.string().url().optional(),
		VITE_APP_TITLE: z.string().min(1).optional(),
	},
	runtimeEnv: import.meta.env,
	emptyStringAsUndefined: true,
});

export function getApiBaseUrl(): string {
	return env.VITE_API_URL ?? "";
}
