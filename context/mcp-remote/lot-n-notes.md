# Lot N — `inkboard-net` : décisions et API

Crate natif seulement (`#![cfg(not(target_arch = "wasm32"))]`, dépendances conditionnelles). `iroh` 1.4 (`default-features = false`,
`tls-ring`, préréglage `Minimal`). **Toutes les commandes cargo : `+1.99.1`** (MSRV du dépôt relevé à 1.91).

## API
- `IrohTransport::host(cfg)` (lie le point de terminaison, accepte) / `::guest(cfg)`. Implémente `Transport` ; `accept()` sans blocage.
  `connect_ticket(&Ticket)` (bloquant, borné par `connect_timeout`) ; `Transport::connect(id)` n'a que l'identité (pas d'indices : préférer le ticket).
  Hôte : `ticket(&HostKeys, Role)`, `hints()`, `local_id()`, `relay_online()`. Un petit runtime tokio est possédé par le transport
  (ne pas appeler `host/connect/ticket` depuis un contexte async ; garder le transport vivant tant que ses connexions servent).
- `IrohConn : Connection` : `send/recv` sans blocage ; `close()` vide la file puis ferme ; `close_cause() -> Option<Error>` ;
  `refuse()` (côté hôte, l'invité voit `Network{Refused}`). Les trames reçues avant la fin restent lisibles.
- `NetConfig { relays, idle_timeout = 120 s, connect_timeout = 30 s, ready_timeout = 10 s, max_frame, loopback_only }` ;
  `NetConfig::loopback()` = sans relais, hôte lié à 127.0.0.1 (tests, deux processus sur une machine).
  `RelaySetting::{Default (relais n0), Custom(liste), Disabled}`, `default_relay_urls()`.
- `diagnose_relay(url, timeout) -> Diagnosis { host, reachable_tcp, tls_trusted, http_status, server_header, gateway, summary }`,
  `GatewayKind::{Zscaler, Other}`, `classify_gateway` (pure).

## Décisions
- **Format du ticket inchangé** : le champ `Ticket.relays` porte les *indices d'adresse* : URL de relais (`https://…`) ou adresse directe
  (`ip:port`) ; `host` = identité iroh. Max 8 indices au total (limite de B). Indices inconnus ignorés.
- **Ajout additif dans `inkboard-session`** : `Error::Network { cause: NetCause, detail }` + `NetCause::{RelayUnreachable, HostUnreachable,
  TimedOut, Refused, Congested, Local}` (`Closed` existant pour la fermeture simple). Rien d'autre ne change.
- Flux : 1 connexion QUIC et 1 flux bidirectionnel par invité ; l'invité écrit un préambule `IBN1` (sinon l'hôte ne voit pas le flux) ;
  trames `u32 LE longueur | octets` ; ALPN `inkboard/session/1`. Préambule absent sous 10 s ou faux : connexion fermée « refusée ».
- Relais invité : `Disabled` = aucun ; `Default` = relais du ticket, sinon ceux de n0 ; `Custom` = liste + relais du ticket
  (l'URL du ticket est arbitraire : à examiner à la revue de sécurité du lot E). Point d'extrémité invité mis en cache par jeu de relais.
- Cause d'échec de connexion : relais attendus mais aucun relais joint -> `RelayUnreachable`, sinon `HostUnreachable` (le délai de
  connexion dépassé en fait partie) ; `TimedOut` = connexion établie restée silencieuse 120 s.
- Fermeture : le dernier émetteur attend que le pair ferme (≤ 5 s) pour ne pas perdre de trames.
- `diagnose_relay` : aucune dépendance HTTP ; `rustls` + `webpki-roots` déjà dans l'arbre via iroh (0 paquet ajouté, vs ~10 pour `ureq`).
  Certificat non fiable (inspection TLS) : nouvel essai sans vérification, signalé par `tls_trusted = Some(false)`. Ni proxy système ni
  magasin de certificats de l'OS.

## Tests
`cargo +1.99.1 test -p inkboard-net` : boucle locale (trames, 2 invités, fermeture, identités, `SecureChannel`, mauvaise clé, hôte
disparu, relais injoignable, tickets) et diagnostic (serveur HTTP local). Test réel : `-- --ignored public_relay` (passé à la main).

## Non vérifié
Chemin TLS de `diagnose_relay` (testé en HTTP local seulement) ; vraie passerelle Zscaler ; reprise après coupure réseau ; charge 20/50 invités.
