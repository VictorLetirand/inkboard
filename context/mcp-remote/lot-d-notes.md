# Lot D — invité natif : décisions et points pour l'intégration (lot I)

## Où est quoi
- `session_guest.rs` : `GuestSession` (machine d'état réseau autour du `Client` optimiste), analyse du lien, nom d'affichage, diagnostic,
  et `impl App` (miroir, capture des modifications, présence). `join_ui.rs` : fenêtre « Rejoindre », bandeaux, message d'erreur.
- `inkboard-core/editor.rs` : crochet séparé en fin d'`impl` : `record_ops`/`take_recorded_ops`, `set_read_only`, `replace_document`, `bump_revision`.
- `inkboard-session` (ajouts) : `Welcome.host_name` + `Welcome.participants` (défaut vide), `Message::Participants`, `Message::Undo/Redo`,
  `Client::request_undo`, `Client::edit` accepté en `Joining` après un premier instantané (reconnexion). Les `Welcome { .. }` du lot C
  doivent recevoir ces deux champs.

## À brancher au lot I
- `app.join.factory` : `Box<dyn Fn(Ticket, &str) -> GuestSession>` ; construire avec `GuestSession::with_defaults(link, diagnoser, ticket, name)`.
  `link` = tout `Transport` (trait `GuestLink` par blanchiment) ; le transport réel doit renvoyer `ConnectError { kind: NoNetwork | TimedOut | Other }`
  (le blanchiment pour un `Transport` met toujours `Other`). `diagnoser` = `Arc<dyn RelayDiagnoser>` (même forme que `inkboard-net`) ;
  `NoDiagnosis` tant que rien n'est branché. `GuestConfig.relay_url` = relais à diagnostiquer si le ticket n'en porte pas.
- Une `Connection` est « ouverte » quand la poignée de main du transport est faite (`is_open()`) ; 20 s sans l'être = échec.
- **L'hôte doit répondre `Ping` par `Pong`** (le bandeau « Reconnexion… » vient de là : sonde à 1 s, bandeau à 2,5 s de silence, nouvelle
  connexion à 20 s) et envoyer `Welcome` puis `Snapshot` à l'acceptation (l'attente de l'accord n'a pas de délai).
- Hôte : `join_url(&ticket)` donne le lien `inkboard://join/<ticket>`. `Participants` doit être renvoyé à chaque arrivée/départ.
- Les noms viennent de `Welcome.participants` / `Participants` ; la présence reçue doit porter `participant` (rempli par l'hôte).

## Décisions
- Le tableau de l'invité est un `Editor` miroir ; l'ancien tableau est mis de côté (`LocalBoard`) et rendu en quittant. Pas d'autosave
  ni de surveillance de fichier ni de serveur MCP en session invité. Chaque page garde la vue de l'utilisateur lors d'un rafraîchissement.
- Un geste local est capturé quand l'éditeur l'applique (journal d'`Op`), converti par `ops_to_wire` et envoyé en un `Submit`. Le miroir
  n'est rafraîchi qu'entre deux gestes ; tant qu'un geste est en cours, les mises à jour de l'hôte attendent.
- Lecture seule : l'éditeur refuse toute modification (pas seulement l'interface), outils limités à Main/Laser, boutons grisés.
- Pas d'envoi d'images par l'invité (le protocole n'a pas de téléversement) : image, bibliothèque d'icônes, formule, dépôt et collage
  d'image sont désactivés en session. Images de l'hôte : demandées par empreinte, 4 à la fois, au moment de l'instantané ; une image
  ajoutée après l'instantané n'est connue qu'au prochain instantané.
- Annuler/rétablir = `Message::Undo/Redo` (activés après une modification confirmée ; l'hôte décide).
- Erreur réseau : `NetClass` = `Offline` (la couche transport dit « pas de réseau »), `Blocked` (passerelle reconnue ou 401/403/407/451/511),
  `HostUnreachable` (le service répond bien), `Unreachable` (générique). Le diagnostic tourne sur un fil ; message provisoire immédiat.

## Non vérifié
- Aucun transport réel, aucun essai avec deux processus ; enregistrement du schéma vérifié seulement sur la configuration et les
  modèles NSIS/WiX (pas d'installeur construit, rien testé sur deb/dmg). Un second lancement `inkboard://…` ouvre une nouvelle instance
  (pas de transmission à l'instance déjà ouverte).
- Captures : `cargo +1.99.1 test -p inkboard-app capture_lot_d -- --ignored` (`lot-d-*.png`).
