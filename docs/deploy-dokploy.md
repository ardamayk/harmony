# Deploy with Dokploy

This runbook runs the Music Server on [Dokploy](https://docs.dokploy.com/) with the Music Server's own sign-in ([ADR 0042](adr/0042-owner-sign-in-in-the-music-server.md)), and gives every pull request a throwaway preview that `.github/workflows/preview-e2e.yml` tests. It assumes Dokploy is already installed on the server and connected to GitHub through a GitHub provider (Dokploy's own GitHub App).

## Layout

One Dokploy project, `harmony`, with two environments:

| Environment | Application | Source | `/data` | Domain | Preview deployments |
| --- | --- | --- | --- | --- | --- |
| `production` | `harmony` | Docker image `ghcr.io/ardamayk/harmony-server:main` | named volume | e.g. `harmony.example.com` | **off** |
| `dev` | `harmony-dev` | GitHub, branch `main` | **no mount** | e.g. `harmony-dev.example.com` | on |

Previews must never be enabled on the production application. A preview reuses its parent application's mounts ([Dokploy issue #4311](https://github.com/Dokploy/dokploy/issues/4311)), so a preview of `harmony` would open the production database and Managed Storage, and an unmerged migration would change them for good.

Production runs the image CI publishes, which exists only once CI has passed on `main`; `harmony-dev` and its previews build from GitHub on the server, which is what previews need.

`harmony-dev` has no mount for `/data` on purpose. Each of its containers, and each preview's, gets a fresh anonymous volume from the Dockerfile's `VOLUME /data` and starts with an empty library. The consequence: **everything in `harmony-dev` and in a preview is thrown away on every redeploy**, imports included. Use them to try changes, never to keep music. Old anonymous volumes stay on disk until you remove them; `docker volume prune` deletes the unused ones.

So there is something to play, CI imports a small demo library after each deploy: two Albums of synthesized melodies with covers and synced lyrics (`scripts/seed-demo-library.ts`). The Preview E2E workflow seeds the preview before its test; the Seed dev workflow waits for `harmony-dev` to come back empty after a push to `main`, then seeds it, and can be run by hand. From your machine: `SEED_URL=https://dev.aneomol.uk SEED_BASIC_AUTH=preview:… mise run seed:demo` (needs ffmpeg). Albums already there are skipped.

## Both applications

1. Create the application in its environment. `harmony`: provider Docker, image `ghcr.io/ardamayk/harmony-server:main`. `harmony-dev`: provider GitHub, this repository, branch `main`, build type Dockerfile, Dockerfile path `server/Dockerfile`, build context the repository root; it builds the Rust Music Server and the Web Client on the Dokploy server, and the first build takes a long time.
2. Choose how the password reaches the server. With a secret manager under Dokploy → Settings → Secrets (Infisical, Vault and others), put `AUTH_USERNAME` and `AUTH_PASSWORD` in it and reference them in step 4; Dokploy fetches them at each deploy. Otherwise, or to keep the clear password out of the container's environment, make a hash instead. The command reads the password from stdin, so it stays out of your shell history:

   ```sh
   docker run --rm -i ghcr.io/ardamayk/harmony-server:main hash-password
   ```

   Type the password, then press Ctrl-D. From a checkout, `cargo run -q -p earthly-music-server -- hash-password` does the same.
3. Find the address range Traefik reaches the application from:

   ```sh
   docker network inspect dokploy-network --format '{{range .IPAM.Config}}{{.Subnet}} {{end}}'
   ```

4. Set the environment:

   ```sh
   SERVER_ADDR=0.0.0.0:8090
   AUTH_USERNAME=${{vault.<provider>.AUTH_USERNAME}}
   AUTH_PASSWORD=${{vault.<provider>.AUTH_PASSWORD}}
   TRUSTED_PROXIES=10.0.1.0/24   # the subnet from step 3
   ```

   `<provider>` is the secret provider's name in Dokploy. With a hash instead, set `AUTH_USERNAME=owner` and `AUTH_PASSWORD_HASH='$argon2id$v=19$...'`, the hash in single quotes.

   Add any optional setting from the [Configuration](../README.md#configuration) table or `docker-compose.yml` that you use, such as `LASTFM_API_KEY`, `SENTRY_DSN` or the External Source. `DATABASE_PATH` and `MANAGED_STORAGE_PATH` already point into `/data` in the image. For Listen Together add `GUEST_ADDR=0.0.0.0:8091` and `PUBLIC_URL=https://<guest domain>`; with sign-in on, the Guest Listener may bind a non-loopback address so Traefik can reach it.
5. Production only: under Advanced → Volumes, add a volume mount with a name such as `harmony-data` and mount path `/data`. `harmony-dev` gets none. Also set the Swarm update order to `stop-first`: Dokploy runs applications as Swarm services, and a `start-first` update would briefly run two servers on one SQLite database.
6. Add the domain with container port `8090`, path `/`, and HTTPS on or off as described under [Networking](#networking). With Listen Together, add the guest domain with container port `8091`.
7. Deploy, and check `https://<domain>/api/v1/health`: `capabilities` should include `auth.session.v1`. Then sign in from the Web Client.

## Preview deployments (`harmony-dev` only)

Dokploy builds a preview for every pull request to the application's branch. It names it `preview-<appName>-<random>`, serves it on the wildcard domain, and posts a pull-request comment headed "Dokploy Preview Deployment" that it edits with the status and URL. Pushing to the pull request redeploys it; closing or merging removes it. See [Dokploy's preview deployment docs](https://docs.dokploy.com/docs/core/applications/preview-deployments).

In `harmony-dev` → Preview Deployments, enable them and set:

- **Wildcard domain:** `*.example.com`. A preview of `harmony-dev` becomes `preview-<appName>-<random>.example.com`.
- **Port:** `8090`. Dokploy's default is 3000, where nothing listens.
- **HTTPS:** as for the domains under [Networking](#networking).
- **Environment:** the same as `harmony-dev`. Both sign in with the public demo account, username `demo` and password `demo`:

  ```sh
  AUTH_USERNAME=demo
  AUTH_PASSWORD_HASH='$argon2id$v=19$m=19456,t=2,p=1$R1avyoy9xsdAbZgOG5/GAg$JCsXrWjoCMGEaVf7t5WPin4cQpX21Ulrkz2Qm0nBGa0'
  ```

  Anyone can read this password, so keep Traefik basic auth (below) in front of `harmony-dev` and its previews, and never use the demo account on a server you keep: make your own hash before deploying one.
- **Require collaborator permissions:** keep it on. This repository is public, and Dokploy warns that previews on a public repository let others run builds on your server; with this on, only authors with write access trigger one.
- **Labels:** optional. With labels set, only pull requests carrying one of them get a preview.
- **Preview limit:** the default of 3 previews at a time is enough.

To keep `harmony-dev` and its previews out of sight, add basic auth in `harmony-dev` → Advanced → Security with a username and a secret password of your own. Previews inherit it from their next deploy. Traefik checks it and removes the `Authorization` header before the Music Server sees it, so the browser asks twice: Traefik's dialog, then the Music Server's sign-in. The Desktop Client cannot connect to such a server, because its Basic header never reaches the Music Server.

Then add the repository secrets `PREVIEW_USERNAME` and `PREVIEW_PASSWORD`: the Traefik basic auth username and password from the Security tab, in clear text under Settings → Secrets and variables → Actions. The Preview E2E workflow waits for Dokploy's comment, signs in to the preview and signs out again. Until previews are set up it finds no comment and ends with a notice, so it never blocks a pull request.

## Networking

### Plain DNS (recommended)

Point an `A` record for each hostname and a wildcard `*` record at the server, turn HTTPS on for the domains and the preview settings, and let Traefik get Let's Encrypt certificates. A wildcard here is served one certificate per preview hostname, so a new preview waits for its certificate. With Cloudflare DNS, keep these records **DNS only** (grey cloud):

- Cloudflare's proxy and Tunnel limit a request body to 100 MB on the free plan, which a large FLAC upload passes; without them there is no such limit.
- Traefik sees the visitor's address itself, so `TRUSTED_PROXIES` is just the `dokploy-network` subnet and sign-in attempts are counted per real address.

The cost: the server's address is public and nothing filters traffic before Traefik. The Music Server's own sign-in and attempt limits are what protect it.

### Behind a Cloudflare Tunnel

`cloudflared` runs on the host and nothing listens on the internet directly.

1. In the tunnel's public hostnames, route each hostname to Traefik at `http://localhost:80`: the production hostname, the `harmony-dev` hostname, and a wildcard `*.example.com` for previews. Traefik picks the application by the Host header, which `cloudflared` passes on.
2. Set HTTPS **off** on every Dokploy domain and on the preview settings. TLS ends at Cloudflare, and Traefik serves plain HTTP to the tunnel. Turn on Cloudflare's "Always Use HTTPS": the session cookie is `Secure`, so signing in works only over `https://`.
3. Use a single-level wildcard such as `*.example.com`, not `*.preview.example.com`. Cloudflare's free Universal SSL certificate covers the domain and its first-level subdomains only ([limitations](https://developers.cloudflare.com/ssl/edge-certificates/universal-ssl/limitations/)).
4. Other hostnames on the same domain keep working: an exact DNS record wins over the wildcard ([wildcard records](https://developers.cloudflare.com/dns/manage-dns-records/reference/wildcard-dns-records/)), and `cloudflared` uses the first ingress rule that matches, so keep the wildcard below the specific hostnames ([ingress rules](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/configuration-file/)).

Traefik believes the `X-Forwarded-For` that `cloudflared` sends only from the addresses in its entry point's `forwardedHeaders.trustedIPs` ([entry points](https://doc.traefik.io/traefik/reference/install-configuration/entrypoints/)). Without that, the Music Server sees every visitor as the tunnel, and the per-address limit on sign-in attempts becomes one limit shared by everyone. To keep visitors apart, add the address `cloudflared` reaches Traefik from to that list in the Traefik configuration Dokploy manages.

### Either way

The Music Server's sign-in faces the internet directly. A gate in front, such as Cloudflare Access, adds a second layer; the Desktop Client then also needs that gate's headers.

## Clients after the switch

- The Web Client shows its sign-in screen. On a phone, sign in again in the installed PWA.
- The Desktop Client: in Settings → Music Server, test the connection, then enter the username and password it asks for. They are saved as an encrypted `Authorization: Basic` header; saving replaces the headers saved before, such as Cloudflare Access ones.

## Move the existing production server

The current production server runs from `/opt/harmony` with Docker Compose (`network_mode: host`, named volume `app-data` at `/data`), a server-local `harmony-auto-deploy.timer` that pulls the image every 5 minutes, and a tunnel hostname pointing at `localhost:8090`. Work through this in order.

- [ ] Pick a quiet time. **Stopping or restarting the server cancels an active Managed Import**; check that nothing is uploading or importing.
- [ ] Stop automatic deploys: `sudo systemctl disable --now harmony-auto-deploy.timer`.
- [ ] Note the settings in `/opt/harmony/.env` and `docker-compose.override.yml`; they move into the Dokploy environment.
- [ ] Stop the stack (`docker compose stop server` in `/opt/harmony`) and back up the whole volume, `app.db*` and `managed/`, with the commands under [Back up and roll back](../README.md#back-up-and-roll-back). Copy the archive to another machine. The music is several gigabytes, so this takes minutes.
- [ ] Find the volume's real name with `docker volume ls`; Compose prefixes it with the project name.
- [ ] Create `harmony` in Dokploy as above, with previews off. For `/data`, either mount the old volume by that name, which keeps the data where it is, or mount a new volume and copy the backup into it before the first start.
- [ ] Deploy `harmony` and check `/api/v1/health` through Dokploy's domain.
- [ ] Point the production (and guest) hostnames at Traefik: with plain DNS, replace their tunnel records with DNS-only `A` records to the server; behind the tunnel, route them to `http://localhost:80`.
- [ ] Sign in from the Web Client, check that the library and its music are there, and set up the Desktop Clients as above.
- [ ] Remove the Compose stack only when the new one has run for a while (`docker compose down` without `-v` keeps the volume). Keep the backup.

From now on Dokploy deploys production. To keep following `main`, redeploy `harmony` when a new `:main` image is published, through Dokploy's deploy webhook or its `application.deploy` API; Dokploy pulls the image again on every deploy. Check for an active import first, as the old timer did: a restart cancels it.
