# T4 - Diagnostic du relais derrière un réseau filtré (page web)

*2026-10-23, branche `fix/web-relay-diagnosis` (depuis 157cd2a). Captures : `screenshots/t4/`. Banc : `crates/inkboard-app/web/test/e2e.mjs`.*

## Le bug observé
Edge, PC d'entreprise, proxy Zscaler à inspection TLS : la page se charge, rejoindre échoue avec « le service répond, mais l'hôte est injoignable » et un diagnostic `Reachable: yes ... Diagnosis: the relay answers (connected by the browser) ... Error: cannot reach the host: no answer within 15 s`. Le même lien marche depuis un téléphone en 5G.

## Cause (établie en laboratoire, pas sur Zscaler)
`dial_failure` (wire.rs) jugeait « relais atteint » si `Endpoint::addr()` contenait une URL de relais. Or iroh y met le relais **dès qu'il est choisi** (en navigateur : après une sonde HTTPS `/ping` en CORS), connecté ou non (doc d'iroh : « merely selecting a relay URL is not sufficient »). Puis `WebDiagnoser` classait « relais atteint » pour toute erreur autre que `RelayUnreachable`/`Local`.
**Reproduit à l'identique** (avant correctif, banc `e4`) avec un faux relais HTTPS local qui répond `/ping` (CORS) mais refuse l'upgrade WebSocket (403) ou ne répond jamais : message « Le service de connexion répond, mais l'hôte est injoignable », diagnostic mot pour mot celui du terrain (`Reachable: yes`, `no answer within 15 s`). Capture « avant » : `screenshots/t4/before-e4-refuse-false-message.png`.
Conséquence sur le terrain : sur ce réseau, le HTTPS vers le relais passe (au moins `/ping` avec en-têtes CORS) mais le WebSocket n'a jamais abouti. La cause réseau exacte (WebSocket filtré, en-tête `Upgrade` retiré, timeout du proxy, inspection TLS qui casse l'upgrade...) reste **inconnue**.

Les blocages « silencieux » purs (DNS qui répond, paquets perdus : port local qui accepte sans répondre, ou `10.255.255.1`) donnaient déjà la bonne classe avant correctif : aucun relais n'est choisi, `addr()` est vide. Seul le cas « HTTPS passe, WebSocket non » était faux.

## Correctif
- `inkboard-net/src/relay_signals.rs` (pur, testé en natif) : croise **sonde HTTPS** (`fetch` no-cors `GET <relais>/generate_204`, délai court, démarrée `probe_delay` = 2 s après le début de la tentative, donc aucune sonde si la connexion réussit vite), **état réel du relais d'iroh** (`Endpoint::home_relay_status()` : `is_connected`, `last_error`) et résultat du dial -> `Cause` : (a) rien ne passe -> relais injoignable (message d'entreprise existant) ; (b) HTTPS passe mais iroh non connecté -> **WebSocket bloqué** (nouveau) ; (c) iroh connecté -> hôte injoignable (message existant). `Local` -> pas de réseau.
- `wire::dial_failure` (natif et web) utilise l'état de connexion d'iroh et non plus la présence de l'URL.
- `WebTransport::connect_ticket_observed` renvoie les `Signals` d'une tentative échouée ; `session_net_web::WebDiagnoser` les convertit en `Diagnosis { reachable (= relais connecté), websocket_blocked, facts }`.
- Nouvelle classe `NetClass::WebSocketBlocked` : message dédié FR/EN qui ne prétend pas connaître la cause, 3 pistes (1. demander à IT d'autoriser les connexions WebSocket vers `*.relay.n0.iroh.link` port 443 ; 2. autre réseau ; 3. « Copier le diagnostic » et l'envoyer à IT). Relais inconnu (lien d'autrui) : seule la piste « autre réseau ».
- Le diagnostic copié contient maintenant : navigateur (user agent), URL du relais dans le lien et utilisée par le navigateur, résultat de la sonde (réponse opaque / échec / délai dépassé + durée) pour les deux écritures du nom, état de connexion du relais d'iroh (+ dernière erreur), durée avant abandon, cause retenue. Rien de secret (ni ticket, ni clé ; test dédié).

## Point final du nom d'hôte (`…iroh.link.`)
- Origine : **iroh lui-même** (`iroh/src/defaults.rs` : `EU_RELAY_HOSTNAME = "euc1-1.relay.n0.iroh.link."`, FQDN avec point pour éviter les domaines de recherche). L'hôte le met dans le ticket ; ce n'est ni notre code ni une normalisation de `url`. Tous les clients iroh natifs l'utilisent, y compris celui qui marche en 5G.
- Cause plausible du blocage ? **Aucune preuve.** Un proxy qui compare le nom d'hôte ou classe l'URL par catégorie pourrait traiter `host.` autrement que `host`, mais le terrain montre que `/ping` en HTTPS passait (donc le nom pointé était reconnu) : le point final n'explique pas à lui seul un WebSocket qui échoue.
- Mesure prise (inoffensive, testée ici) : l'invité **navigateur** utilise le nom **sans** point final (`without_final_dot`, test unitaire ; relais réel n0 : connexion établie et banc complet vert avec cette écriture) ; le diagnostic sonde aussi l'autre écriture et l'affiche, pour que le prochain rapport dise si un seul des deux noms passe. L'hôte natif et le ticket sont inchangés.
- Non testable ici : Zscaler. On ne peut donc pas dire si la normalisation change quoi que ce soit chez l'utilisateur.

## Banc (`e2e.mjs`)
`--only <préfixes>` pour lancer quelques scénarios ; `window.__inkboard_test.set_timeouts(connect_ms, probe_ms, probe_delay_ms)` (build `web-test` seulement) raccourcit les délais (8 s / 3 s / 1 s) ; aucun test n'asserte une durée.
Nouveaux scénarios : **e3** blocage silencieux (port local qui accepte sans répondre ; adresse non routable `10.255.255.1`) ; **e4** HTTPS passe / WebSocket refusé (403) ou jamais répondu (faux relais HTTPS local, certificat auto-signé via `openssl`, `--ignore-certificate-errors`), FR, téléphone ; **y** relais réel, hôte parti ; **e1** étendu (diagnostic du blocage DNS). Run complet : **89 vérifications, 0 échec** (65 existantes + 24 nouvelles).

## Prouvé / non prouvé
Prouvé : le faux « Reachable: yes » et le faux message se reproduisent quand HTTPS passe et le WebSocket non, et disparaissent avec le correctif ; trois situations (relais injoignable, WebSocket bloqué, hôte absent avec relais connecté) donnent trois messages et trois diagnostics distincts, dans Chrome 155, en FR/EN et sur téléphone émulé ; la table de croisement des signaux est testée de façon déterministe.
**Non prouvé** : le comportement derrière un vrai Zscaler (inspection TLS, 403 de blocage, WebSocket coupé) - non reproductible ici ; Edge (seul Chrome essayé) ; que la cause terrain soit bien « WebSocket filtré » plutôt qu'un autre échec après un `/ping` réussi (le diagnostic de la prochaine tentative le dira) ; l'effet de la normalisation du point final chez l'utilisateur ; le faux relais ne reproduit pas l'inspection TLS (certificat ignoré par option du banc).

## Pièges
- Les relais n0 sont écrits avec un point final dans les liens ; une règle de blocage doit lister les deux écritures (banc : `MAP *.iroh.link …, MAP *.iroh.link. …`).
- Le banc doit servir `/ping` avec `Access-Control-Allow-Origin` : sans cela iroh ne choisit aucun relais et on retombe dans le cas « rien ne passe ».
- Un `.exe` de test lancé par le banc verrouille le binaire : pas de `cargo test` pendant un run du banc.
- `tests_session_e2e::a_guest_leaving_frees_its_place_and_the_host_can_share_again` a échoué une fois sous charge (CPU partagé), vert 3 fois seul et en run complet : non lié.

## Commandes
```
cargo +1.99.1 fmt --all ; cargo +1.99.1 clippy --workspace --all-targets -- -D warnings ; cargo +1.99.1 test --workspace
CC_wasm32_unknown_unknown=<clang> AR_wasm32_unknown_unknown=<llvm-ar> cargo +1.99.1 build -p inkboard-app --target wasm32-unknown-unknown --no-default-features --features web-glow
PUPPETEER_DIR=<puppeteer-core> CC_...=<clang> AR_...=<llvm-ar> node crates/inkboard-app/web/test/e2e.mjs --out context/mcp-remote/screenshots/t4   # complet, ~10 min ; --only e3,e4,y --skip-build pour les nouveaux cas
```
