---
status: accepted
---

# Owner sign-in in the Music Server

[ADR 0038](0038-owner-access-through-a-signing-proxy.md) opened the Owner's listener to the internet only behind an identity proxy that signs every request, and rejected sign-in in the Music Server itself. That works for the maintainer, but asks every other self-hoster to run and configure a signing proxy before they can open their library from a phone. It also rules out the usual container hosting, such as Dokploy behind Traefik, where the server cannot stay on loopback. So the Music Server gets its own username and password sign-in. The signing proxy keeps working for those who choose it; the two are alternatives.

## How it works

- `AUTH_USERNAME` and `AUTH_PASSWORD_HASH` are set together or not at all, and never together with the `OWNER_*` settings of ADR 0038. The hash is an argon2id PHC string made by `earthly-music-server hash-password`, which reads the password from stdin so it stays out of shell history and process lists. Unset, the server behaves as before.
- Since 2026-11-04, `AUTH_PASSWORD` may replace `AUTH_PASSWORD_HASH` (one or the other, never both): the password in clear text, as a secret manager such as Infisical through Dokploy hands it over. The server hashes it at startup and keeps the stored hash while the password still matches it, so a restart ends no session. The trade-off is that the clear password is in the container's environment, where anyone with Docker access to the host can read it; `AUTH_PASSWORD_HASH` stays for operators who want it kept out.
- `TRUSTED_PROXIES` lists the addresses or CIDR ranges whose `X-Forwarded-For` the server believes. Without it, the client address is the connection's own.
- With sign-in on, a loopback Host no longer makes a request the Owner's, and `SERVER_ADDR` may bind any address, as a container behind a reverse proxy needs.
- Every `/api` route except health, login, logout and the API reference (`/api/docs`, `/api/openapi.yaml`) then needs either the `harmony_session` cookie or `Authorization: Basic` with the same username and password, and answers `401 unauthenticated` without one. The response has no `WWW-Authenticate` header, so a browser never shows its own login dialog. The static Web Client and the docs stay public; they hold no library data.
- `POST /api/v1/auth/login` sets the cookie: HttpOnly, SameSite=Lax, Secure unless the request came to a loopback address, valid for 30 days and renewed while in use. Sessions are stored hashed in SQLite, so signing out ends one on the server, and a stolen database holds no usable session.
- The Desktop Client sends Basic credentials through the per-server encrypted request headers ADR 0038 introduced; scripts send them too.
- Failed attempts, through login or Basic, are counted per client address (an IPv6 address by its /64 prefix, which one customer usually holds whole). After 5 failures, each further attempt waits twice as long as the one before, up to 15 minutes, and is refused with `429 too_many_attempts` and `Retry-After` until then. No account is ever locked, and there is no per-username counter: the username is not secret, so either would let anyone keep the Owner out of sign-in. A Basic header that was already refused is answered from memory for 5 minutes without counting again, so a Desktop Client still sending an old password does not use up its own address's attempts. The counters live in memory and are bounded in size; once full, new addresses share one counter rather than pushing old ones out, so a flood of addresses can neither exhaust memory nor earn fresh free attempts. A restart clears them.
- The users table holds one user, the existing default user id, whose username and password come from the settings above. This keeps room for per-user libraries later; they are planned but not part of this decision.
- The Guest Listener of [ADR 0037](0037-listen-together-rooms-on-the-owners-music-server.md) stays separate, on its own address. Moving Listen Together onto one listener, with an optional separate host for Guests, is a follow-up.

## Why a cookie and not only Basic

The Web Client is installed on phones as a PWA ([ADR 0041](0041-phones-use-the-web-client-as-a-pwa.md)), and iOS home-screen apps handle Basic authentication poorly. A persistent, server-set HttpOnly cookie survives restarts, is out of reach of page scripts, and can be revoked by signing out. SameSite=Lax, together with the existing Origin check on writes, covers cross-site request forgery. Basic remains for clients that cannot keep a cookie jar.

## Considered Options

- Plain Basic authentication for everything: rejected. It fails in the iOS PWA, and a browser offers no way to sign out of it.
- Keep the signing proxy as the only way: rejected as the only way. It is too much setup for most self-hosters, and it ties the server to loopback. It stays as an option.
- An external identity provider through OIDC: rejected. It is heavy for a personal server, and it brings back the dependency on a service that this decision removes.

## Consequences

- This supersedes ADR 0038's rejection of sign-in in the Music Server. The rest of ADR 0038 stands for operators who choose the signing proxy.
- Without a signing proxy in front, the server's own sign-in faces the internet directly, and its password check, attempt limits and session handling are the only barrier. Operators may still put Cloudflare Access or a similar gate in front for a second layer.
- Every client needs credentials once sign-in is on, including a Desktop Client on the same machine, because loopback no longer counts.
- The per-address limit is only as good as the address: behind a proxy that is not listed in `TRUSTED_PROXIES`, or that does not pass the client address on, every client looks like the proxy and the per-address limit becomes one shared limit. Spreading guesses over many addresses is not slowed beyond each address's own limit; the argon2id cost and a strong password are the defence there.
- Choosing a strong password is the Owner's job; the server does not enforce a policy on it.
