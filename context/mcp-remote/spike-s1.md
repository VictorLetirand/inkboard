# Spike S1 — transport : `iroh` convient-il ? (2026-10-21)

Prototype jetable dans `spikes/s1-iroh/` (hors workspace racine). Résultats bruts dans `spikes/s1-iroh/results/`.

## VERDICT : **iroh retenu** (sous réserve du MSRV, voir « Points d'attention »)

| Critère du plan (§9) | Résultat | OK |
|---|---|---|
| Le navigateur se connecte via relais | Chrome 155 : relais public n0 **et** relais local, 1 000 messages + 2 x 5 Mio échangés avec l'hôte natif | oui |
| Latence médiane d'un message < 150 ms sur relais public | aller-retour médian **50,7 ms** (navigateur), **48,5 ms** (natif) ; p95 54,9 / 50,2 ms | oui |
| WASM < 6 Mo avant compression | **2 730 298 o (2,60 Mio)**, gzip -9 : 1 008 620 o (0,96 Mio), sans `wasm-opt` | oui |
| Licences compatibles MIT OR Apache-2.0 | toutes permissives ; deux licences hors liste actuelle de `deny.toml` (voir plus bas) | oui, avec ajout |
| API utilisable sur les versions testées | `iroh` 1.4.0 (série 1.x, « stable » selon la politique de versions) ; compile et fonctionne du premier coup natif et wasm | oui |

## Versions testées
`iroh` 1.4.0, `iroh-relay` 1.4.0 (binaire, `cargo install iroh-relay --version 1.4.0 --features server --locked`), `noq` 1.4.0, `wasm-bindgen` + CLI 0.2.129, rustc **1.99.1** (épinglé dans le spike), Chrome 155.0.8059.80 (headless, `puppeteer-core`), Windows 11, Python 3.13 (`http.server`). `iroh` utilisé avec `default-features = false, features = ["tls-ring"]`, préréglage `Minimal` (pas de publication DNS : le ticket porte les adresses).

## Protocole du prototype
Ticket = identité de l'hôte + adresses (URL de relais et/ou adresses directes) + secret de 32 octets, postcard + base32 : 148 à 227 caractères, lu dans le fragment `#inkb…`. Un flux bidirectionnel QUIC par invité : `Hello(secret, id participant)` → `Welcome(resumed)`, 1 000 `Ping/Pong` séquentiels de 32 o, `BulkDown`/`BulkUp` de 5 Mio, `Bye`. L'hôte refuse un mauvais secret. Deux invités natifs simultanés + une session de contrôle par mode réseau.

## Mesures (ms ; RTT = aller-retour applicatif, 1 000 échantillons ; débit 5 Mio, Mbit/s)
| Scénario | connexion | poignée de main | RTT médian | RTT p95 | débit ↓ / ↑ | chemin |
|---|---|---|---|---|---|---|
| Natif, boucle locale directe | 3,1 | 1,3 | 0,08 | 0,14 | 2 077 / 2 168 | ip |
| Natif, relais local (HTTP) | 12,0 | 13,9 | 0,20 | 0,35 | 498 / 549 | relais |
| Natif, relais public n0 (1 invité seul) | 134 | 82 | 49,1 | 51,1 | 16,8 / 15,2 | relais |
| Natif, relais public n0 (2 invités en même temps) | 131 / 133 | 90 / 82 | 48,5 / 48,8 | 50,2 / 50,7 | 11,6 / 6,6 et 5,6 / 14,4 | relais |
| Chrome, relais local | 116 | 12 | 1,2 | 2,3 | 49,5 / 65,8 | relais |
| Chrome, relais public n0 | 225 | 78 | 50,7 | 54,9 | 16,6 / 15,2 | relais |

- Établissement hôte : 57 ms (direct), 0,3 s (relais local), 1,1 s (relais public : `bind` + `online`). Invité natif 30–50 ms, navigateur 13 ms (hors chargement du wasm : 45 ms).
- Mode « hybride » (relais n0 + adresses directes dans le ticket) : chemin `ip` dès la poignée de main, RTT 0,15 ms (boucle locale/LAN/Tailscale de ce poste).
- **Reconnexion** : processus invité tué sans fermeture QUIC, puis nouveau processus avec le même id de participant : l'hôte répond `resumed=true` (table d'ids côté prototype). `bind` + `connect` + poignée de main : ≈ 30 ms (direct), ≈ 260 ms (relais public). Côté hôte, la coupure sans fermeture n'est vue qu'après **≈ 15 à 31 s** (délai d'inactivité QUIC : 15 s, 31 s, 30 s sur 3 essais, direct puis relais local) : la reprise doit se fonder sur l'id de participant, pas sur la détection de fermeture.

## Ce qui a marché
- (a) direct en boucle locale ; (b) relais public n0 avec **hôte et invité sans socket IP** (`clear_ip_transports`) et ticket réduit à l'URL du relais (`https://euc1-1.relay.n0.iroh.link./`). Preuve de relayage : journaux invité `connecting relay_url=Some(…euc1-1…) ip_addresses=[]` puis `Dialing relay by websocket dial_url=wss://euc1-1…/relay` (`results/native/relay-n0-debug-evidence.txt`), et côté hôte `paths=[*relay Relay(https://euc1-1…)]` (chemin sélectionné) ; (c) relais auto-hébergé local.
- Navigateur : page statique (`web/index.html`) lit `location.hash`, appelle `run_guest_wasm`, rejoint l'hôte natif via relais. Le journal du serveur `http.server` ne contient **aucune** occurrence du ticket (0 sur 13 requêtes GET) : le fragment n'est pas envoyé.
- Les relais publics limitent le débit : message d'`iroh` « The relay is rate-limiting this endpoint; outbound relay traffic is being throttled » observé (invités natifs et navigateur) ; débit relayé public 6–17 Mbit/s ici. Les messages courts ne sont pas affectés (RTT stable, max 62 ms).

## Ce qui a échoué ou gêné
1. `iroh` 1.0 à 1.4 exigent **rustc ≥ 1.91** ; MSRV d'Inkboard = 1.90 (`stable` local = 1.90.0). `cargo +1.90.0 check` : `error: rustc 1.90.0 is not supported by the following packages: iroh@1.4.0 requires rustc 1.91`.
2. wasm : `ring` compile du C pour `wasm32` : `error occurred in cc-rs: failed to find tool "clang"`. Contournement : clang du NDK Android (qui contient la cible wasm32) via `CC_wasm32_unknown_unknown` (`build-wasm.sh`). Il faut aussi `getrandom` avec `--cfg getrandom_backend="wasm_js"` (`.cargo/config.toml`) et un CLI `wasm-bindgen` de version identique au crate.
3. Relais local `--dev` : écoute `[::]:3340` ; sur ce poste Windows `127.0.0.1:3340` était refusé. Symptôme côté hôte : `endpoint did not reach a relay within 30 s`, sonde `tcp connect error … refusée`. Correctif : `http_bind_addr = "127.0.0.1:3340"` (`relay-dev.toml`).
4. Edge : lancement via `puppeteer-core` échoué (`Failed to launch the browser process: Code: 0`), non investigué.

## Auto-hébergement du relais : étapes réelles et difficultés
Étapes : `cargo install … --features server` (le binaire exige la fonctionnalité `server`), fichier TOML, `iroh-relay --dev -c relay-dev.toml`, `RelayMode::custom([url])` côté hôte ; l'invité dérive son relais du ticket. Compilation et démarrage sans accroc hors point 3. Difficultés : (i) la doc d'auto-hébergement est signalée « maintenue par la communauté » et renvoie au code (`main.rs`) pour la liste des options ; (ii) en production : nom DNS + TLS (Let's Encrypt, ports 80/443 ; UDP 7842 pour la découverte d'adresse, facultatif) ; (iii) les limites de débit sont **désactivées par défaut** (`[limits.client.rx]` à configurer) ; (iv) une page en `https://` ne pourra pas joindre un relais en `ws://` (règle du contenu mixte, **non testée**) : relais public ou auto-hébergé avec TLS obligatoire pour le web ; (v) rien d'installé en service (VPS, TLS, charge non essayés).

## Licences (`cargo metadata` + `cargo deny`, cibles Windows, Linux, macOS, wasm32)
≈ 270 paquets résolus (toutes fonctionnalités). Plus de 80 % `MIT OR Apache-2.0` ou `MIT`/`Apache-2.0`. Autres : `Unicode-3.0` (18, dont ICU), `BSD-3-Clause` (dalek, subtle…), `ISC` (`rustls-webpki`, `untrusted`), `Apache-2.0 AND ISC` (`ring`), `Zlib`, `BSD-2-Clause`, `CC0`/`MIT-0` (au choix). `cargo deny check licenses` avec la liste du dépôt échoue sur seulement : **`CDLA-Permissive-2.0`** (`webpki-roots`, `webpki-root-certs`, données des racines Mozilla, aussi en natif) et **`Unlicense`** (`ws_stream_wasm`, `pharos`, `async_io_stream`, wasm seulement). Tous permissifs, compatibles avec MIT OR Apache-2.0 ; D6 impliquerait d'ajouter ces deux licences à `deny.toml`. Avis `cargo deny check advisories` : aucune vulnérabilité ; 3 « unmaintained » (`paste` via `netwatch`, `bincode` et `anymap2` via `tracing-subscriber-wasm`, outillage du spike seulement).

## Documentation officielle (docs.iroh.computer, lue le 2026-10-21)
- Navigateur : compilation wasm supportée, **toutes les connexions passent par un relais** (pas de perçage UDP) ; `default-features = false` obligatoire ; pas de paquet npm fourni ; chiffrement de bout en bout conservé.
- Relais publics : « développement et loisirs seulement », pas de SLA, débit limité par connexion (valeurs non publiées), seule la dernière version stable est supportée, relais seulement pour la dernière version majeure ; le relais voit IP, heures et volumes. Alternatives : relais partagés/dédiés (Iroh Services).
- Auto-hébergement : binaire, image `n0computer/iroh-relay`, configuration TOML (Let's Encrypt automatique).
- Stabilité : série 1.x ; majeure ≥ 6 mois, mineure ≥ 4 semaines ; protocole rétrocompatible avec la majeure précédente ; **support complet d'une mineure 3 mois**, d'une majeure 1 an : il faudra suivre les mises à jour mineures.

## Recommandation D1 (relais)
Option (a) pour développement et bêta, **avec** relais configurable (lot E) et option (b) planifiée avant toute diffusion large : les invités navigateur dépendent toujours d'un relais, les relais publics sont officiellement hors production et limités en débit (6–17 Mbit/s mesurés). Le ticket porte l'URL du relais : basculer vers un relais à soi ne change ni le protocole ni la page. Le trafic de contrôle (messages courts) tient largement dans la limite ; les images volumineuses (15 Mio) seront les premières touchées.

## Points d'attention
- **MSRV** : adopter `iroh` oblige à passer le MSRV d'Inkboard de 1.90 à 1.91+ (et le `stable` local) : décision à prendre avec D6.
- **Sécurité** : l'invité construit son relais à partir du ticket (URL arbitraire) ; à examiner dans la revue de sécurité du lot E. Le prototype vérifie le secret dans le canal QUIC ; l'AEAD applicatif du plan n'est pas essayé.
- CI : le wasm demande un `clang` avec la cible wasm32 (courant sous Linux, **non vérifié** ici).

## NON vérifié
- Un seul poste, un seul réseau, **un seul relais public** (`euc1-1`) : rien depuis un autre pays, un NAT réel, un VPN ou un réseau d'entreprise ; le perçage de NAT n'est pas essayé (« direct » = boucle locale et interfaces locales de ce poste).
- Navigateurs : seulement Chrome 155 headless (puppeteer). Pas Edge (échec de lancement), Firefox, Safari, mobile, ni Chrome avec fenêtre. Page servie en `http://127.0.0.1`, jamais en `https://` publique.
- Latence aller simple non mesurée (horloges non synchronisées ; RTT seulement). Messages de 32 o séquentiels ; pas de flux multiples, perte de paquets, ni plus de 2 invités natifs + 1 navigateur (pas de test à 20 ou 50).
- Relais auto-hébergé : ni TLS, ni Let's Encrypt, ni VPS, ni charge, ni limites de débit configurées. Limite exacte des relais publics inconnue.
- La reprise (`resumed`) est une logique du prototype (id de participant), pas une reprise de session `iroh` ; la coupure testée est un arrêt de processus, pas une coupure réseau.
- `wasm-opt` non utilisé (taille wasm sans optimisation binaryen) ; build wasm non essayé sur Linux/CI.
- Licences : métadonnées uniquement, textes non relus ; les 270 paquets ne sont pas tous exercés par notre usage.

## Reproduire (depuis `spikes/s1-iroh/`)
`cargo build --profile native --features native` ; `bash run-native.sh direct|relay-n0|relay-local|hybrid-n0` ; `bash run-cut.sh direct` ; relais : `.tools/bin/iroh-relay --dev -c relay-dev.toml` ; wasm : `CLANG_BIN_DIR=… bash build-wasm.sh`, puis `python -m http.server 8080 --directory web`, `host --net relay-n0 --ticket-file t` et `node web/drive.mjs http://127.0.0.1:8080/index.html t`.
