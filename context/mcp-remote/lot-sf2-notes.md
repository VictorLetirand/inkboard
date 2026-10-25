# Lot SF2 — durcissement de l'hôte et du protocole : décisions

Commandes `cargo +1.99.1`. Tests : `-p inkboard-session` (`tests/snapshot.rs`), `-p inkboard-net` (`loopback`), `-p inkboard-app tests_session` (blocs « lot SF2 » en fin de `tests_session_host.rs` / `tests_session_guest.rs`).

## Protocole : version 2 (changement incompatible)
- `PROTO_VERSION` 1 -> 2 : `Message::SnapshotPart { seq, part, last, head?, pages }`. Un instantané qui tient dans ~192 Kio part tel quel
  (`Message::Snapshot`), sinon il est coupé entre objets en parties numérotées (une page peut s'étendre sur plusieurs parties, même `id`).
  `snapshot::into_messages` (hôte) et `SnapshotAssembler` (invité : tolère doublons et désordre, une partie 0 d'un `seq` plus récent
  recommence, une partie d'un `seq` plus ancien est ignorée, remis à zéro à la coupure). Un hôte v1 / un invité v1 sont refusés (message de version).
- Cause de S-04 : `IrohConn` refuse > 1 Mio + 1 Kio dans les deux sens ; on ne change pas cette limite, on découpe.

## Corrigé (constat -> où)
- S-05 : `Limits.sync_*` (1/s, rafale 3) par participant, lecteurs compris, pour `Resync` et `Hello` répété (violation puis exclusion). Un `Hello`
  d'un membre qui a déjà prouvé qu'il a le document (`Submit`/`Undo`/`Resync`) est ignoré ; avant, répondu dans la limite du budget (Welcome perdu).
- S-04 : voir ci-dessus + `send` renvoie un booléen ; échec d'envoi = `fail_conn` (connexion fermée, place gardée, `HostEvent::Warned`).
  `SimNet::set_max_frame` simule la limite du vrai transport.
- S-08 : `lock_conflict` (hôte) : Put/Delete/Order/SetLayers/RemovePage d'un invité touchant un objet/calque verrouillé ou changeant `locked`.
- S-12 : `check_id` sur `editing`, `Transient.state.id == object` (hôte et invité), octets d'éphémères/s (`ephemeral_bytes_*`).
- S-11 : connexions muettes comptées à part (16 max, les plus anciennes partent après 2 s de grâce, délai de poignée de main 10 s) ;
  `HostConfig.pending_ms` (120 s) : demande en attente refusée (`Declined`, pas de nouvelle raison pour ne pas toucher `join_ui.rs`) même avec des `Ping`.
- S-13 : limiteur de trames/octets par participant AVANT le décodage (`Inbound::Flood`), budget de 8 Mio lus par `poll` (le reste attend), départ tournant.
- S-10 : `inkboard-net` : files bornées en octets (32 Mio, `Congested` côté émission, contre-pression côté réception), au plus 64 poignées de main
  en cours ; hôte : morceaux d'images en file <= 32 Mio par invité, réserve d'images <= 64 Mio, quota lié à l'identité de transport (`quotas`) +
  plafond total (`total_asset_quota_bytes`, 512 Mio).
- S-09 : `Client` valide Snapshot/parties/Applied (`Limits::host_trust`, mêmes contrôles qu'un invité, comptes relevés), erreur = session fermée, tableau
  intact ; `Presence/Transient/StrokePoints/Agent/Participants` invalides sont jetés ; `future` <= 1 000 entrées et 32 Mio ; noms/listes assainis
  (`names::clean_name`, 256 participants max, 100 cibles d'agent).

## Non fait / non vérifié
- Piles d'annulation par auteur (S-10, `inkboard-core/history.rs`, hors liste de fichiers).
- Une trame unique > 1 Mio (un objet énorme de l'hôte, un `Applied` de l'hôte) échoue encore : l'invité est coupé puis reprend via instantané.
- `Hello` d'un membre : seul le cas « document jamais confirmé » reçoit une réponse (borné) ; transport iroh réel non essayé avec 10 000 objets
  (SimNet avec la limite de trame réelle seulement). Pas de plafond par adresse (S-11), pas de bouton « tout refuser ».
