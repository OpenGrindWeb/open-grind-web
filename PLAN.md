# Open Grind Web — plan

Objectif : utiliser Open Grind sur iPhone sans Mac ni réinstallation, en servant l'interface web depuis un VPS (Pterodactyl) et en l'installant comme PWA (« Sur l'écran d'accueil » dans Safari).

Base : fork de https://git.opengrind.org/open-grind/open-grind (commit `d5f9105`, 2026-11-10).

## Architecture

```
iPhone (Safari PWA)
   │ HTTPS + cookie de session
   ▼
Reverse proxy (Caddy) ── domaine public, TLS, rate-limit
   │
   ▼
og-server (Rust, axum) ── conteneur Pterodactyl
   ├─ sert le build statique SvelteKit (déjà en SPA, ssr = false)
   ├─ POST /ipc/:command   → remplace invoke() de Tauri
   ├─ GET  /events (WebSocket) → remplace les events Tauri (chat, statut)
   ├─ GET  /media/*        → remplace le schéma ogmedia://
   └─ crate `grindr` + `wreq` (BoringSSL) → API Grindr
```

Le front ne change pas du tout : og-server injecte `server/src/bridge.js` en tête de `index.html`. Ce script définit les globales que lit `@tauri-apps/api` (`__TAURI_INTERNALS__`, `__TAURI_OS_PLUGIN_INTERNALS__` avec la plateforme `ios`, `__TAURI_EVENT_PLUGIN_INTERNALS__`, `isTauri`). Les commandes natives (géoloc, presse-papiers, haptique, updater…) sont traitées dans le navigateur, les événements arrivent par le WebSocket `/events`, et tout le reste part en `POST /ipc/<commande>`. Le build du front reste identique à l'amont, ce qui garde les merges triviaux.

## Couplage Tauri mesuré

- 77 commandes Rust, 67 fichiers référencent `tauri::`.
- Le cœur est petit : `api/rest.rs` (237 lignes), `api/ws.rs` (271), `api/auth.rs` (412), `state.rs` (un seul `GrindrClient`).
- Le reste est natif plateforme (updater 14 commandes, push 19, haptique, desktop entry, scroll, apparence) et sera remplacé ou désactivé.

## Périmètre MVP

Inclus :
- Connexion email + mot de passe, restauration de session
- Grille / profils / filtres (REST générique `request`)
- Chat temps réel (WebSocket)
- Photos : affichage, envoi via `<input type="file">` (remplace le media picker natif)
- Géolocalisation via l'API Geolocation de Safari (HTTPS requis)
- Stockage des secrets via le file store existant (`storage/file_store.rs`), chiffré au repos

Exclu du MVP (phase ultérieure) :
- Notifications : Web Push (VAPID) alimenté par `push_poll` côté serveur
- Connexion Google / Facebook (dépendent d'une extension WebKit embarquée)
- Updater, haptique, intégrations bureau

## Phases

1. **Fork + squelette serveur** : crate `og-server` (axum) qui réutilise `src-tauri/src/api/*` en extrayant ce qui dépend de `tauri::State` / `AppHandle` derrière un trait. Build statique du front servi par le serveur.
2. **Bridge front** (fait) : `bridge.js` injecté par le serveur, données d'app (`read/write/remove_app_data`) stockées côté serveur.
3. **Auth + REST + chat** (fait, sauf test avec un vrai compte) : porter `auth.rs`, `session_recovery.rs` (`current_session`, `account_restriction`, `sign_in_with_email`, `sign_out`…) et `ws.rs` (`ws_connect`, `ws_send`, `ws_reconnect`). Le MVP fonctionnel en local.
4. **Médias** : route `/media`, upload signé côté serveur.
5. **PWA** : manifest, icônes, `apple-mobile-web-app-capable`, safe-area, test sur iPhone.
6. **Déploiement** : image Docker / egg Pterodactyl, Caddy, sauvegarde du store.
7. **Notifications** (optionnel) : Web Push.

## Sécurité (VPS + domaine public)

Le serveur détient le jeton de session Grindr, l'identité d'appareil et la clé de signature des uploads. Quiconque passe l'authentification contrôle le compte.

- Authentification propre au serveur, obligatoire avant toute route : mot de passe fort + passkey (WebAuthn, supporté par Safari) ou TOTP. Cookie `HttpOnly; Secure; SameSite=Strict`.
- Rate-limit et blocage après échecs (Caddy + fail2ban, ou intégré au serveur).
- Aucune route `/ipc` accessible sans session ; liste blanche des commandes exposées.
- Store de secrets chiffré, hors du dossier servi, sauvegardé.
- Pas de logs des corps de requêtes Grindr.

## Risque IP

Les requêtes Grindr partiront de l'IP du VPS (datacenter) alors que la position vient de l'iPhone. Grindr peut repérer ce décalage. Atténuation possible plus tard : faire sortir le trafic Grindr par une connexion résidentielle (tunnel WireGuard vers une box ou un Pi à la maison).

## Maintenance

Le fork doit suivre l'amont (version 0.1.0-rc, API Grindr qui bouge). Garder les modifications du front limitées au shim et à l'alias Vite pour que les merges restent simples.
