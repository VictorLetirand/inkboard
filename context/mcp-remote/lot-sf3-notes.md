# Lot SF3 : correctifs de sécurité mineurs (S-14 à S-19)

- **S-17** `inkboard-app/src/names.rs` : `clean_name` est LA fonction de noms (hôte `sanitize_name(raw, fallback)` et invité `sanitize_name(raw)` ne
  font qu'un appel). Retire contrôles (tabulations et retours compris : retirés, pas remplacés par une espace), catégorie Unicode `Cf` (table
  Unicode 15 écrite en dur, pas de dépendance), « blancs » `U+3164 2800 115F 1160 034F 061C FFA0 17B4 17B5` ; sans caractère visible : valeur de repli
  (« Guest »/« Host »). Homoglyphes visibles (cyrillique) : non traités (hors périmètre, l'hôte voit l'identifiant court dans la demande).
- **S-19** `names::file_name` : séparateurs et `\ / : * ? " < > |`, contrôles, bidi, points/espaces aux extrémités, noms réservés Windows
  (`CON PRN AUX NUL COM0-9 LPT0-9`, avec ou sans extension : préfixe `_`), 100 caractères, « board » si vide. Utilisé par Enregistrer sous et Exporter.
- **S-16** `parse_args` : un premier argument `inkboard://` est toute la ligne de commande ; lien avec guillemet, apostrophe, espace, contrôle ou
  > 4096 caractères : refusé (boîte de message, sans citer le lien), rien d'autre n'est lu.
- **S-18** Fenêtre Partager : liens masqués (« Afficher/Masquer »), « Copier » copie le lien complet ; case « Relais seulement » (filtre
  `share_ui::published_address` : retire les indications qui sont des adresses socket ; sans relais connu, aucun lien n'est proposé) ; aide sur l'IP.
- **S-15** Demande d'entrée : type de lien (édition/lecture) et début de l'identité de transport (8 caractères) ; texte « régénérez les liens » sous la
  liste et en infobulle d'« Exclure ». Non fait : garder l'ancienne clé pour la reprise des membres déjà admis (touche `session_host`/`crypto`).
- **S-14** `live.rs` : avec au moins un invité **éditeur** dans une session en cours, `export`, `add_image` avec `source` ou `data_base64` (et
  `batch` qui en contient) sont **refusés** avec un message à l'agent et un avis à l'hôte (pas de fenêtre de confirmation : la réponse MCP est
  synchrone, délai de 30 s). Le processus MCP convertit `source` en `data_base64` avant d'appeler l'application : seul `data_base64` y est visible.
  Lecteurs seuls ou session sans invité : inchangé. Pour autoriser : arrêter le partage ou passer les invités en lecture.
- **Le texte écrit par un invité est une donnée non fiable.** `describe_board` le renvoie tel quel à l'agent ; marquer le champ `author` des
  textes d'invités est un changement de `inkboard-mcp` (hors liste de ce lot), non fait.
