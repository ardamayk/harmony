---
status: accepted
---

# Client errors through the Music Server

[ADR 0033](0033-opt-in-sentry-for-the-rust-programs.md) left the Web Client and the Electron renderer without Sentry: they send only timings, through `POST /api/v1/client-metrics`. So a React error, a failed promise or the root error screen reaches no one. The owner wants these in their own Sentry, without setting a DSN on every installed Desktop Client and without a client ever talking to Sentry or any other third party.

So the clients run the Sentry browser SDK, and the Music Server carries its errors to Sentry, **only when `SENTRY_DSN` is set**.

## How it works

- With `SENTRY_DSN` set, health on the Owner's listener advertises `client-errors.v1`. The Guest Listener never does and has no route for it.
- The Web Client, and with it the Electron renderer, loads `@sentry/browser` only after it sees `client-errors.v1`, so a server without Sentry costs the page nothing. Its transport posts each envelope to `POST /api/v1/client-errors` through the same API client as every other request, so the Desktop Client's connection and sign-in apply.
- The client has no DSN. The SDK needs one to send at all, so it gets a placeholder on `music-server.invalid`; nothing is sent there.
- The Music Server reads the envelope (at most 256 KiB), keeps only its error events and sends each with its own Sentry client and DSN, so `before_send` scrubs them exactly like its own (ADR 0033's "What is never sent"). The envelope's headers, its DSN among them, the client's environment, and every other item (sessions, transactions, attachments, client reports) are dropped. Events keep the SDK's platform, the release `earthly-web@<Music Server version>` and the tag `client` (`web` or `desktop`).
- The router's root error screen reports the error it caught, since React never lets it reach the window's error handler.

## What the client does not collect

On top of the server's scrubbing, the SDK is told to collect nothing about the person or their requests (`dataCollection`: no user, cookies, headers, bodies, query parameters or stack frame variables), and runs without the session and request-context integrations. Of the breadcrumbs it keeps only requests and page navigations, whose URLs the server reduces to route words and `<id>`; clicks and console lines are dropped, because a button's title or a log line can name what is playing. No tracing, replay or feedback is started.

## Considered Options

- A DSN in the Desktop Client's environment, sent straight to Sentry: rejected. Every installation would need it set, and the client would talk to a third party.
- Errors as JSON through a route of our own, like the metrics: rejected. Stack traces, linked errors and breadcrumbs would mean rewriting the SDK.
- The SDK's `tunnel` option to the Music Server: rejected for a custom transport, because in the Desktop Client requests go through the core's proxy and the saved server's credentials, which the API client already handles.
- Forwarding the envelope unread: rejected. The server could not scrub it, and a client could send any item to the owner's Sentry.

## Consequences

- JavaScript errors land in the same Sentry project as the Music Server's, told apart by platform, release and the `client` tag.
- Stack frames point at the built, minified files; source maps are not uploaded, so frames read as bundle positions until that is added.
- A Desktop Client's release names the Music Server's version, which may differ from the renderer's own build.
- The Electron main process is not covered; the Desktop core keeps its own Sentry from ADR 0033.
