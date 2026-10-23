# Lot F2 - transport iroh du navigateur, côté invité (notes)

*2026-10-23, branche `feat/remote-f2` (depuis 88783f8). Guides : `spike-s1.md` (iroh en wasm), `lot-f1-notes.md` (base web). Pas d'interface ici (lot F3).*

## Ce qui est fait
- **`inkboard-net` compile pour `wasm32-unknown-unknown`** : `iroh` (mêmes features que le spike : `default-features = false, tls-ring`), `tokio` (feature `sync` seule : canaux), `n0-future` (temporisations, `or`), `wasm-bindgen-futures`, `getrandom 0.3` + `wasm_js`.
- **Code partagé extrait (sans tokio runtime ni natif)** : `wire.rs` (ALPN, préambule `IBN1`, trames `u32 LE | octets`, lecture/écriture de trame, correspondance erreurs QUIC -> `Error`/`NetCause`, relais de l'invité `GuestRelays`, `dial_plan`, `ticket_addr` (filtrage des relais du ticket), `open_stream`) et `cancel.rs` (`CancelToken`). `transport.rs` (natif) les utilise : comportement inchangé (tests `loopback`, `relay_policy`, e2e de l'app verts). `config.rs` et `ticket.rs` étaient déjà purs : réutilisés tels quels (seuls `RelaySetting::{mode,is_disabled}` sont natifs).
- **`web_transport.rs`** (wasm) : `WebTransport::guest(NetConfig)` (un endpoint gardé pour toute la session, relais seul), `connect_ticket` / `connect_ticket_cancellable` (`async`, délai `connect_timeout`), `close`. `WebConn` implémente `Connection` (`send`/`recv` non bloquants, un lecteur et un écrivain `spawn_local`, mêmes limites que le natif : 1024 trames / 32 Mio, `Congested`, trame trop grande), fermeture avec cause (`close_cause` : `TimedOut` après 120 s de silence, `Refused`, `Closed`). Erreurs : `RelayUnreachable` (aucune connexion au relais), `HostUnreachable`, `TimedOut`, `Refused`, `BadTicket` : les mêmes variantes que le natif.
- **App** : `session_link.rs` (partagé : `connect_error`, `Dialing<C>` = connexion en cours d'établissement, extrait de `session_net.rs`) ; `session_net_web.rs` = `guest_factory()` (WebTransport + `Dialing`, tâche `spawn_local` à la place du thread) ; `WebDiagnoser` : une page ne peut pas sonder le relais, elle sait seulement si le transport l'a atteint (atteint -> même diagnostic que « le relais répond, l'hôte est injoignable » ; sinon message générique pare-feu/passerelle du plan §5). `platform_web` utilise maintenant `inkboard_net::config::{check_relay_url, is_known_relay}` (même politique de relais que le bureau, le shim qui refusait tout est supprimé). `session_guest.rs` : un seul changement, le diagnostic ne passe plus par `std::thread::spawn` en wasm (réponse immédiate). `web.rs` branche la fabrique (la fenêtre « Rejoindre » n'est plus inerte, mais son lien/nom/écran d'erreur restent à faire en F3).
- **Banc de bout en bout** `crates/inkboard-app/web/test/e2e.mjs` (+ `README.md`, `npm run e2e`) : construit la page avec la feature `web-test` (`window.__inkboard_test.{join,leave,state}`, module `web_test.rs`, jamais dans la page publiée), lance l'hôte natif (test `#[ignore]` `tests_web_host::web_bench_host` : 5 objets, relais public n0, accepte les demandes, `add`/`quit` sur stdin), ouvre `index.html#<ticket>` dans Chrome.
- **Licences** : `THIRD_PARTY_LICENSES.md` régénéré avec la cible `wasm32-unknown-unknown` ajoutée à `scripts/gen_third_party.py`, à `deny.toml` (`targets`) et au test `release_metadata.rs` (+14 crates : wasm-bindgen*, js-sys, web-sys, gloo-timers, ws_stream_wasm, pharos, async_io_stream, send_wrapper, wasm-streams, bumpalo…). `cargo deny check licenses sources bans` : ok (`Unlicense` et `CDLA-Permissive-2.0` déjà autorisées).

## Prouvé (cette machine, Windows 11, Chrome 155 headless, relais public n0, Internet)
`node e2e.mjs` : 12 vérifications passent. Session `live` ; `Welcome` (nom de l'hôte) ; snapshot : 5 objets chez l'hôte = 5 chez l'invité ; `add` côté hôte -> 6 objets dans le navigateur ; un lien invalide est refusé avec message ; `leave` ; le ticket n'arrive jamais au serveur statique (fragment) ; aucune erreur console.
Gates : `cargo +1.99.1 fmt --all` ; `clippy --workspace --all-targets -D warnings` ; `test --workspace` (tout vert) ; `build -p inkboard-app --target wasm32-unknown-unknown --no-default-features --features web-glow` (et `+web-test`) ; `cargo deny check licenses sources bans`.
Taille de la page (release, `wasm-opt -Os`, glow) : .wasm 18,16 Mio brut / 7,66 gzip / 4,71 brotli (F1 : 12,49 / 5,19 / 3,49 ; iroh + TLS ring ajoutent ~5,7 Mio brut, 2,5 Mio gzip) ; budget du plan (< 15 Mo compressés hors icônes) tenu.

## Non prouvé / limites
- Pas d'essai : Firefox, Safari, Edge, téléphone, page en `https://` publique, `web-wgpu`, hôte derrière passerelle d'entreprise (seul le chemin « relais atteint / non atteint » est mappé ; le 403 de la passerelle n'est pas lisible depuis une page).
- Pas de test automatisé des cas d'échec réseau côté navigateur (relais bloqué, hôte disparu, refus, coupure 120 s) : seules les variantes d'erreur sont partagées avec le natif, testées en natif. À faire en F3/F4 avec l'écran d'erreur.
- Reconnexion : la logique est celle de `session_guest` (même endpoint, donc même identité), non exercée dans le navigateur. Pas d'écouteur `online`/`offline` du navigateur vers `network_changed`.
- Le banc a besoin d'Internet et du relais public n0 (limité en débit) ; il n'est pas dans `cargo test`.
- Pas de persistance du nom/relais en web (`platform::PERSISTS = false`, inchangé).
- `clippy --target wasm32` : `drop_non_drop` de `inkboard-core/src/serial.rs` et `explicit_auto_deref` de `platform_web.rs:87` existaient avant (non touchés).

## Pièges / décisions
- **`ring` compile du C pour wasm32** : il faut un `clang` avec la cible WebAssembly (LLVM >= 15) pour `trunk build` / `cargo build --target wasm32-unknown-unknown`. Ici : `CC_wasm32_unknown_unknown` / `AR_wasm32_unknown_unknown` = `clang.exe` / `llvm-ar.exe` du NDK Android (`...\ndk\28.2.13676358\toolchains\llvm\prebuilt\windows-x86_64\bin\`). **La CI aura besoin de clang** (Ubuntu : paquet `clang`, non vérifié ici). Documenté dans `docs/WEB.md` et `docs/fr/WEB.md`.
- **`getrandom` 0.3** exige `--cfg getrandom_backend="wasm_js"` : mis dans `.cargo/config.toml` (`[target.wasm32-unknown-unknown] rustflags`). Définir la variable d'environnement `RUSTFLAGS` la remplace : y répéter le drapeau. Aucun effet sur les builds natifs ni sur `cargo test --workspace`.
- `WebTransport` prend `&self` (endpoint dans un `RefCell`, jamais emprunté à travers un `await`) pour être partagé par `Rc` entre le lien et les tâches de dial. Le natif reste bloquant (`connect_ticket` bloque) : les deux API diffèrent forcément par `async`.
- La page n'embarque pas de hook en production : `web-test` est une feature, absente de `Trunk.toml`.
- Le banc lance l'hôte via l'exécutable de test de `inkboard-app` (`cargo test --no-run --message-format=json` pour le trouver) : pas de binaire livré en plus.
- `node_modules` hors du dépôt (le test du français parcourt tous les dossiers) : `PUPPETEER_DIR=<dossier de puppeteer-core>`.

## Commandes
```
# build page (depuis crates/inkboard-app/web) ; CC/AR = clang et llvm-ar avec la cible wasm32
RUSTUP_TOOLCHAIN=1.99.1 trunk build --release --features web-glow,web-test --dist dist-e2e
# banc complet (build + hôte natif + Chrome)
cd crates/inkboard-app/web/test && PUPPETEER_DIR=<...>/puppeteer-core node e2e.mjs      # --skip-build, --headed
# gate wasm
cargo +1.99.1 build -p inkboard-app --target wasm32-unknown-unknown --no-default-features --features web-glow
# hôte seul (pour essayer à la main)
cargo +1.99.1 test -p inkboard-app web_bench_host -- --ignored --nocapture
```

## Pour F3 (interface)
Tout est prêt côté réseau : `app.join.factory` est branché en web ; il reste lire `location.hash` (`parse_join_link` accepte déjà `https://…#ticket` et le ticket nu), pré-remplir la fenêtre, le nom, et l'écran d'erreur (les `NetIssue` sont déjà produites). `window.__inkboard_test` montre comment l'état de la session est lu (`web_test.rs`).
