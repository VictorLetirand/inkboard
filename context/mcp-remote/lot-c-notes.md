# Lot C — hôte de session : décisions et API

Tout est testé sur `SimNet` ; aucune dépendance à `inkboard-net`. Commandes `cargo +1.99.1`.

## Cœur (`inkboard-core`)
- `Editor::apply_external(&[Op])` et `apply_external_with(|doc| -> Vec<Op>)` : hors pile d'annulation locale, révision +1.
- `history::AuthorHistories` (pur) : une pile par auteur (`record/undo/redo/note/forget`). Annuler restaure l'état « avant » **sauf** ce qui a changé
  depuis : une opération n'est défaite que si son état « après » est encore l'état courant (`holds`). Le reste est `Skipped{object, by}`.

## `session_host.rs`
- `SessionHost::start(cfg, &Editor, Box<dyn HostTransport>)` ; `poll(&mut Editor, now_ms)` à chaque image. `HostTransport` = `accept()`
  (blanket impl pour tout `Transport` : le trait de B a un type associé, pas d'objet-trait).
- `SessionNet::start() -> (Box<dyn HostTransport>, HostAddress)` : à implémenter par le lot I (iroh). Appelé seulement par « Démarrer le partage ».
- Hôte : le `Sequencer` garde une copie autoritaire ; les changements de l'hôte (y compris l'agent MCP et son Ctrl+Z) sont trouvés par
  `diff_documents(copie, editor)` quand la révision change, puis diffusés (auteur = hôte). Pas besoin du crochet de capture du lot D.
- Plafond = hôte compris (défaut 10) ; places gardées 60 s ; 1 demande en attente par identité de transport ; exclusion = bannissement de l'identité.
- Rôle accordé = min(clé du lien, choix de l'hôte). Violation (limite, débit, quota) : avertissement (`Rejected{client_seq:0}` pour les non-modifications),
  exclusion après `max_strikes` (20, fenêtre 30 s). Image : envoyée par morceaux avant le `Submit`, `asset` = SHA-256 de l'image ; type sniffé
  (png/jpeg/gif/webp/svg), quota par invité, jamais décodée par l'hôte avant affichage.
- Noms : `sanitize_name` (contrôles et marques de direction retirés, 1–40), `unique_name` (« Anna (2) »), fichier `display_name`.

## Ajouts à `inkboard-session` (additifs)
`ParticipantInfo`, `Welcome.host_name/participants` (serde default), `Message::Roster`, `Message::Undo/Redo`, `Sequencer::insert_asset`,
deux bras dans `client.rs::handle` (Roster en `Relay`, Undo/Redo parmi les messages réservés à l'hôte). Trois littéraux `Welcome` de tests complétés.

## Interface (`share_ui.rs`)
Bouton « Partager » + pastille/compteur (carte en bas à droite), fenêtre Partager, liste des participants (renommer, droit, suivre, exclure),
fenêtre d'accord, curseurs distants dessinés par `AgentPresence::paint` (champ `remote`, faute d'accès à `tools.rs`).

## Limites connues
- Instantané en un seul message (les canaux invités doivent accepter de grands cadres) ; pas de reprise d'image au milieu (on redemande).
- Pas d'historique par auteur pour l'agent MCP (ses changements sont attribués à l'hôte et suivent sa pile) ; badge « édite le texte de… » non fait.
- Références visuelles Linux (`tests/snapshots`) à régénérer : la carte en bas à droite a un bouton de plus.
