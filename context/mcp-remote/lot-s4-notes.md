# Lot S4 : durcissement restant

Commandes `cargo +1.99.1`. Tests : `-p inkboard-core author_tests`, `-p inkboard-session --test session`, `-p inkboard-app tests_session_host tests_presence tests_session_images`, `-p inkboard-net --test loopback`.

## Corrigé (constat -> où -> test)
- **S-15** `session_host.rs` : `regenerate_links` garde les clés révoquées (`revoked_keys`, 16 jeux au plus). Elles ne servent qu'à la **reprise** d'une place par une identité
  de transport déjà admise (`Conn.revoked_key`, preuve vérifiée avec cette clé) ; un nouveau venu ou un exclu qui présente un ancien lien n'obtient aucune réponse.
  Les demandes en attente sont refusées (`BadProof`). Un exclu reste refusé (`Banned`), plus rien ne lui est envoyé. Tests :
  `new_links_cut_off_newcomers_and_waiting_requests_but_an_admitted_guest_resumes_its_place`, `a_removed_guest_neither_reads_nor_comes_back_…`.
  Accepté : un exclu qui change d'identité et garde le lien repasse par la fenêtre de consentement tant que les liens ne sont pas régénérés.
- **S-14** `live.rs` (`mark_guest_text`) : `untrusted_text` + `author {guest, untrusted}` sur les objets dont le dernier auteur est un invité ; `session_guest.rs` : le nom d'un
  événement d'agent est affiché « nom (agent) ». Tests : `what_a_guest_wrote_reaches_the_agent_marked_as_untrusted`, `cursors_names_strokes_and_the_agent_…`.
- **Mémoire de l'historique (S-10)** : `inkboard-core/history.rs` (`AuthorHistories::with_budgets`) : 32 Mio par auteur et 128 Mio au total (taille JSON estimée des pas),
  éviction du plus ancien pas de l'auteur qui en tient le plus ; « dernier auteur » de 100 000 objets au plus (la moitié la plus ancienne part). `Sequencer::set_budgets` :
  journal de 64 Mio et 200 000 objets suivis ; un invité qui a besoin d'une entrée évincée reçoit un instantané. Tests déterministes de charge
  (`undo_memory_is_bounded_…`, `the_memory_of_who_modified_what_is_bounded_…`, `the_log_and_the_change_table_are_bounded_…`).
- **Plafond par identité (S-11)** : `HostConfig.max_per_peer` (3). Au `Hello` (après la preuve) : refus net `RefuseReason::TooManyFromPeer` avec message (EN/FR) ; avant toute clé :
  au plus `2 x max_per_peer` connexions ouvertes par identité, l'excédent est fermé sans lecture (aucun message possible, rien n'est connu). Test :
  `one_identity_cannot_hold_more_places_than_allowed_nor_flood_the_host_with_connections`. Pas de plafond par IP : le transport n'expose pas l'adresse.
- **Gestes de l'hôte (ouvert d'après `lot-p-notes.md`)** : `share_ui::send_host_gesture` envoie un objet et les nouveaux points toutes les 50 ms (`SessionHost::send_host_transient/stroke`,
  auteur = hôte) ; le résultat validé les remplace. Tests : `a_stroke_of_the_host_in_progress_…`, `a_move_of_the_host_in_progress_…`.
- **Images de l'hôte après l'instantané (ouvert d'après `lot-g-notes.md`)** : `Message::Assets { list }` (additif, **sans** changement de version : un pair qui ne le connaît pas
  jette la trame) diffusé par `flush_host` avant la modification qui utilise l'image ; l'invité l'ajoute à ses images connues (`learn_announced`, 10 000 au plus) et la demande par
  empreinte. Test : `an_image_the_host_adds_after_the_guest_joined_reaches_it_without_a_new_snapshot`.

## Tests instables : causes racines
- `a_wrong_key_is_refused_by_the_host` (`inkboard-net`) : `IrohConn::refuse` arrêtait l'écrivain (fin de flux, FIN) **avant** `conn.close(REFUSED)` ; selon l'ordre des tâches l'invité lisait
  une fin de flux normale (`Closed`) au lieu du refus. Reproduit : 6 échecs sur 72 (24 processus en parallèle). Correctif : fermer la connexion d'abord. Après : 0 échec sur 240.
- `a_gesture_held_still_is_not_taken_for_an_abandoned_one` (`tests_presence`) : `RemotePresence::now()` = horloge réelle + temps virtuel ; sur une machine chargée le temps réel entre
  deux signes de vie comptait en plus du temps virtuel (délai de sécurité de 3 s). Reproduit en ajoutant 80 ms de sommeil réel par tour. Correctif : `freeze_clock()` (tests seulement) fige
  l'origine, seul `skew` fait avancer le temps. Preuve : 0 échec sur 120 processus en parallèle, et la même série de tests passe avec 80 ms de sommeil par tour.

## Non fait / ouvert
- Trame unique > 1 Mio de l'hôte (S-04, reste), SVG lourd envoyé par un hôte hostile à un invité (S-09), DNS public vers adresse privée pour la connexion `iroh` (S-06).
- Pile d'annulation de l'hôte (`History`, 500 pas) bornée en nombre seulement.
- Pas de bouton « tout refuser » ; « Exclure » ne régénère pas les liens (décision de produit).
- Réseau réel (`iroh`) non essayé pour les nouveautés (SimNet seulement), sauf `a_wrong_key_is_refused_by_the_host`.
