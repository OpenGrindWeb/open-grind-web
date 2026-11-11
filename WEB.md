# Open Grind Web

An unofficial fork of [Open Grind](https://git.opengrind.org/open-grind/open-grind)
that runs the app as a **self-hosted web app**, so it can be used on an iPhone
from the home screen (no Mac, no sideloading, no reinstalling every 7 days).

Not affiliated with the Open Grind project. For the official app, see
[opengrind.org](https://opengrind.org).

## How it works

```
iPhone (home-screen web app)
   │ HTTPS + gate session cookie
   ▼
reverse proxy (Caddy, Nginx Proxy Manager, Traefik…) ── TLS
   │
   ▼
og-server (Rust, axum)
   ├─ serves the SvelteKit build
   ├─ POST /ipc/<command>   stands in for Tauri's invoke()
   ├─ GET  /events          WebSocket, stands in for Tauri events
   ├─ GET  /media/…         photo/video proxy (ogmedia://)
   ├─ POST /stage           uploads from Safari's file picker
   └─ grindr + wreq         talks to Grindr like the desktop app
```

- `server/src/bridge.js` is injected first in `index.html`. It defines the
  globals `@tauri-apps/api` reads, so the front believes it runs in a Tauri
  iOS webview: native-only commands are answered in the browser, the rest go
  to the server.
- The server ports the parts of `src-tauri` the web needs (sign-in, session,
  REST, websocket, media, uploads) and compiles some upstream modules
  unchanged (`video/`, `photo/source.rs`, `photo/encode.rs`, `upload/form.rs`).
- Web-only front changes: iOS-style theme (`src/ios-theme.css`), quick filters
  that fold into a "…" menu, tab transitions, Right Now (upstream PRs 520 and
  523), and English / French / Spanish / German (`src/lib/i18n`).

Not available on the web build: Google / Facebook sign-in, reCAPTCHA-gated
accounts, push notifications, HEIC decoding (Safari hands over JPEG).

## The access gate

The server holds a live Grindr session, so everything sits behind a gate:

- **Authenticator code (TOTP)**: 6 digits, one use per code, per-IP lockout,
  and code logins pause after 30 wrong codes in an hour. On iPhone, the
  Passwords app can fill the code in.
- **Password** (optional): argon2 hash.
- Gate sessions last 30 days and survive restarts (only digests are stored).

With neither configured, the first start generates a TOTP secret, keeps it in
the data directory and shows its QR code once in the console.

## Deploying

### Pterodactyl

Import `server/pterodactyl/egg-open-grind-web.json`, create a server from it
and set:

| Variable | |
| --- | --- |
| `GIT_URL` | this repository's HTTPS URL |
| `GIT_BRANCH` | `main` |
| `GIT_TOKEN` | only for a private fork |
| `OG_TRUST_PROXY` | number of reverse proxies in front (usually `1`) |
| `OG_PASSWORD_HASH`, `OG_TOTP_SECRET` | optional, see above |

Give the server **at least 4 GB of disk**: the build (node_modules, Rust
target) happens on the server's volume, since Wings mounts the installer's
`/tmp` as a small tmpfs, and is cleaned up afterwards (~30 MB stays). The
Rust build also wants 2-3 GB of RAM: raise `docker.installer_limits.memory`
in Wings' `config.yml` if the installer is capped lower.

Install (it builds the front and the server, a few minutes the first time),
start, scan the QR code from the console, and point your reverse proxy at
the allocated port. **Reinstall** updates the app; `data/` is kept. The egg
only bootstraps: the install script itself lives in the repository
(`server/pterodactyl/install.sh`), so updates to it need no new import.

### Docker

```bash
docker build -f server/Dockerfile -t og-server .
docker run -p 8080:8080 -v og-data:/data -e OG_TRUST_PROXY=1 og-server
```

### Environment

| Variable | Default | |
| --- | --- | --- |
| `OG_BIND` | `127.0.0.1:8080` | listen address |
| `OG_STATIC_DIR` | `build` | SvelteKit build |
| `OG_DATA_DIR` | `data` | device identity, sessions, secrets |
| `OG_PASSWORD_HASH` | | `og-server hash-password` |
| `OG_TOTP_SECRET` | | `og-server totp-setup` (prints a QR code) |
| `OG_TRUST_PROXY` | `0` | proxies in front; `X-Forwarded-For` is only trusted from a loopback or private peer |
| `OG_INSECURE_COOKIE` | | `1` for local HTTP testing only |

HTTPS is required in production (cookies, and Safari only allows geolocation
over HTTPS).

## Development

```bash
bun install
OPEN_GRIND_DEMO=1 bunx vite dev   # demo data, no account needed
cd server && cargo test           # needs cmake + clang (BoringSSL)
```

Keeping up with upstream: fetch `upstream` (the official repository) and merge
its `main`; front changes are kept to a few components to limit conflicts.
