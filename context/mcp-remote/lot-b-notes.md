# Lot B — `inkboard-session` : décisions et API

Crate pur Rust (natif + `wasm32-unknown-unknown`). Dépend de `inkboard-core` (non modifié).

## API (modules)
- `proto` : `Message` (JSON, balise `t`), `PROTO_VERSION = 1`, `check_proto`, `Hello{proto,name,join_proof,role_requested,resume}`,
  `Welcome`, `Snapshot` (sans octets d'images + `AssetInfo` par hash SHA-256), `Submit/Applied/Rejected`, `Resync`, éphémères
  (`Transient`, `StrokePoints`, `Presence`, `Agent`), contrôle (`Follow/Kick/RoleChange/Bye/Ping/Pong`).
- `wire` : `WireOp` (`Put/Delete/Order/SetLayers/AddPage/RenamePage/RemovePage/SetBackground/SetName`), `apply_wire(doc, op) -> Vec<Op>`
  (applique et renvoie les `Op` du cœur, dont l'inverse donne un retour arrière exact), `rollback`, `OpEncoder` / `ops_to_wire(ops, doc_avant)`.
- `codec` : trait `Codec` (+ `peek_hello_proto`), `JsonCodec`, `read_hello` (version vérifiée avant le décodage complet).
- `crypto` : `Ticket` / `Link` (`<base>#<ticket>`), `SessionKey`, `HostKeys` (clé édition + clé lecture), `FrameCipher`, `SecureChannel`,
  `join_proof`, `Entropy` (`OsEntropy`, `SeededEntropy` = tests seulement).
- `transport` : traits `Connection` (poll, sans async) et `Transport`; `sim` : `SimNet` (graine, horloge virtuelle, latence, perte,
  duplication, réordonnancement), `SimTransport`, `SimConn`.
- `sequencer` : `Sequencer` (côté hôte : ordre `seq`, dédoublonnage par `client_seq`, `superseded`, `catch_up` -> `Applied`s ou `Snapshot`).
- `client` : `Client` (machine à états pure : `connect/edit/handle/tick/take_outbox/take_events`).
- `limits` : `Limits` (configurable) + fonctions pures `check_*`, `Quota`, `RateLimiter` (horloge explicite). `assets` : hash, manifeste, morceaux.

## Décisions
- **Rôle = clé.** Deux clés de session par session (liens édition et lecture) ; la clé qui ouvre la 1re trame donne le rôle
  (`HostKeys::open_first`), jamais la demande du client. Régénérer les clés révoque les anciens liens.
- **Trame** : `version | nonce(12, aléatoire) | ChaCha20-Poly1305`. Clés dérivées par HKDF-SHA256 et **par sens** (pas de réflexion).
  Nonce aléatoire car la clé est partagée par tous les invités d'un rôle (un compteur entrerait en collision). Rejeu traité par `client_seq`.
- Le ticket (base64url + somme de contrôle de 6 octets) détecte la corruption accidentelle ; l'authenticité vient de l'AEAD.
- **Ajouts au plan** : `SetLayers/SetBackground/SetName` (le cœur a ces `Op`), `AddPage` garde un `index` (pages peu nombreuses, bornées
  à la longueur), `Resync{last_seq}` (trou détecté + resynchronisation périodique pour rattraper une perte en queue),
  `RejectReason::Duplicate{host_seq}`, champ `author` rempli par l'hôte sur les éphémères.
- `Object` n'a pas de champ `version` (cœur intact) : l'hôte suit « dernière modification de chaque objet » dans `Sequencer`
  et signale `superseded` à l'auteur dont l'objet avait changé depuis sa `base_seq`.
- **Client** : annule l'optimiste (inverses des `Op`), applique l'hôte, rejoue la file ; `Rejected{Duplicate}` met l'entrée en
  « parked » jusqu'à ce que `last_seq >= host_seq` (jamais de divergence durable). Hôte qui a oublié l'invité : file abandonnée
  (`PendingDiscarded`) puis nouvel instantané. Snapshot ancien ou répété ignoré.
- Validation : bornes typées (cadres, points, tableaux, tailles de police) + parcours générique du JSON de l'objet (tout nombre borné,
  `width` borné comme épaisseur, total des chaînes) + aller-retour d'encodage (détecte NaN/Inf, que JSON écrit `null`).
- `getrandom` 0.4 avec `wasm_js` en dépendance conditionnelle `wasm32` : compile sans drapeau `RUSTFLAGS`.

## Dépendances ajoutées (licences MIT/Apache-2.0, `cargo deny check -W unmaintained` : advisories, bans, licences, sources OK)
`chacha20poly1305 0.11` (sans `getrandom`), `hkdf 0.13`, `sha2 0.11`, `getrandom 0.4`, `base64 0.23` et `serde*`, `thiserror` (déjà
dans l'espace de travail). Nouvelles transitives : `aead`, `cipher`, `digest`, `hmac`, `poly1305`, `universal-hash`, `crypto-common`,
`hybrid-array`, `block-buffer`, `inout`, `cmov`, `ctutils`, `const-oid`, `typenum`. Avertissements `multiple-versions` : `getrandom` (0.3/0.4).

## Tests
`cargo test -p inkboard-session` : `session` (convergence `proptest` N=2..20 avec perte/dup/réordonnancement, reconnexion, snapshot,
rôles, versions), `wire` (aller-retour `Op`/`WireOp`), `crypto`, `robustness` (octets aléatoires), `validation`, `protocol`.
Build wasm : `cargo build -p inkboard-session --target wasm32-unknown-unknown`.

## Limites connues
- Transport simulé : tests dédiés (`tests/sim.rs`) non écrits (écriture refusée pendant la session) ; la simulation n'est exercée que
  via `session.rs`. Aucun transport réel, aucune vérification dans un vrai navigateur (le build wasm est vérifié, pas l'exécution).
- Codec JSON d'essai (gros pour les tracés) ; pas de compression des `Snapshot`.
