# Lot T3 : test de robustesse (entrées hostiles)

Branche `test/robust` (base `a7b9160`). Commandes `cargo +1.99.1`. Aucun test n'asserte une durée réelle : les tailles sont bornées par construction, le temps de l'hôte se mesure en ticks du SimNet (10 ms virtuelles), la mémoire par un compteur d'allocations.

## Méthode
- **Graine fixe** partout (ChaCha pour proptest, SplitMix pour les générateurs maison) : un passage CI rejoue exactement les mêmes entrées. Chaque banc a une forme courte (CI) et une forme longue `#[ignore]` pilotée par `INKBOARD_FUZZ_CASES` / `INKBOARD_FUZZ_SEED`. cargo-fuzz n'a pas été tenté (chaîne nightly + libFuzzer + sanitizers, peu praticable sous Windows) : les bancs longs en tiennent lieu.
- Les assertions sont toujours les mêmes : pas de panique ni de débordement de pile ; refus explicite (erreur typée, `Rejected`, `Kick`, `Refused`, `isError`) ; état intact (même document chez l'hôte et chez l'invité honnête, aucun identifiant en double, nombres finis) ; l'invité honnête converge et peut encore éditer ; mémoire bornée.
- Mémoire de l'hôte : `peak_alloc` (dev-dependency d'`inkboard-app`, MIT, tests seulement) est un compteur **global au processus**. Le test `memory_stays_bounded_under_a_flood` relance donc le binaire de test en processus fils (`--exact`, un seul thread), y joue une 1re vague de trames hostiles (échauffement : ~6 Mio de tables et caches, une fois pour toutes), puis une 2e vague de même taille et vérifie que l'hôte ne retient **plus rien** (mesuré : +9 Kio). `SimNet::forget_closed` vide ce que le réseau simulé garde pour les connexions fermées (un vrai réseau le libère).
- « L'hôte n'est jamais bloqué plus de X ms » : le temps CPU réel n'est pas mesurable sans horloge réelle. Preuve par bornes : l'hôte lit au plus 8 Mio par `poll` (test existant `a_poll_reads_a_bounded_amount…`), et `an_honest_guest_is_served_in_a_bounded_number_of_ticks_during_a_flood` impose qu'une modification honnête soit appliquée en **au plus 40 ticks (400 ms virtuelles)** pendant que 3 pairs hostiles inondent (rotation de lecture, limiteurs avant décodage).

## Ce qui a été attaqué

| Cible | Fichier de test | Attaques |
|---|---|---|
| Décodage et validation des messages | `crates/inkboard-session/tests/hostile.rs` | une vingtaine de messages valides (tous les types) mutés au niveau JSON (types faux, nombres extrêmes, chaînes de 130 000 caractères, listes de 60 000, imbrications de 120 niveaux, champs retirés/dupliqués), octets abîmés/tronqués, UTF-8 invalide, listes de 400 000 éléments annoncées non livrées, `1e999`, `NaN`, offsets `u64::MAX` ; tout message accepté doit se ré-encoder à l'identique et s'appliquer sans corrompre ni échapper à l'annulation |
| Séquenceur | idem | séquences d'opérations hostiles (ids d'un petit jeu pour provoquer collisions, auto-références, cycles de connecteurs/groupes, calques doublons, pages en double, objets NaN/inf/1e300, `Order` d'ids inconnus), rejeu (doublons refusés), `catch_up` avec `last_seq` ∈ {0, 1, n, n+1, `u64::MAX`} ; réplique rejouée = document de l'hôte (hash) |
| Client (hôte hostile) | idem | snapshots piégés (page en double, `seq` extrême, parties sans fin, sans en-tête, deux fins, après la fin, numéros 4095/4096/`u32::MAX`), `Applied` hors ordre/extrêmes, `Rejected` avec `host_seq` énorme, `Welcome` répétés, `Bye`/`Kick`/`RoleChange`, déconnexions, éditions locales en même temps ; état jamais corrompu, session fermée proprement sur erreur de validation |
| Hôte réel (`SessionHost`) | `crates/inkboard-app/src/tests_hostile_host.rs` | avant admission : `Submit` en première trame, `Hello` répétés, trames de mauvaise clé/vides/tronquées/de 3 Mio ; lecteur qui demande `Edit` ; identité qui revendique la place d'un autre ; opérations sur ids/pages inexistants ; page supprimée par l'hôte pendant l'envoi ; cycles de connecteurs (150 coudes autour des mêmes obstacles) puis édition de l'hôte ; rejeu de trames ; `client_seq` aux bords de `u64` ; envois d'images avec identifiants falsifiés, trous, doublons, offset max, 1000 envois inachevés ; horloge qui saute/recule ; plafond du tableau ; **foule aléatoire** (3 pairs, lecteurs et éditeurs, contenu muté depuis les messages valides, remplacés quand exclus) à côté d'invités honnêtes et de l'hôte qui édite/annule |
| Fichiers atteignables à distance | `crates/inkboard-formats/tests/hostile_files.rs` | SVG : filtres, motifs, masques, `@import`, chemins de fichier/UNC/http, exponentielle de `<use>` (190 `<use>` = 2^95), 20 001 éléments, imbrication, bitmap 65535×65535 intégré, géométrie de 1 Mio+, bombes d'entités ; en-têtes PNG/GIF/JPEG/WebP déclarant 65535² ou `u32::MAX` (tronqués à chaque longueur, octets aléatoires) ; tailles SVG absurdes (1e9, 1e-30, NaN, 65535) ; draw.io : 100 000 balises ouvertes, bombe deflate (72 Mio), entités ; Mermaid : 1100 nœuds, 2600 liens, lignes de 200 000 caractères, sous-graphes ouverts ; `.inkboard` : JSON imbriqué 200 000 niveaux, nombres 1e308, ids en double |
| Serveur MCP | `crates/inkboard-mcp/tests/hostile.rs`, `stdin_limits.rs` | arguments **générés depuis le schéma de chaque outil** (tous les outils de `tools/list`, ~30) puis abîmés (types faux, nombres extrêmes, chaînes géantes et malveillantes, ids inexistants, listes de 2500 éléments, `$N.clé` invalides) ; après chaque appel le fichier se recharge, nombres finis, ids uniques, et **le tableau passe `check_pages(Limits::host_trust())`** (ce qu'un invité accepte) ; lot de 201/5000 opérations ; textes de 1 Mo ; image 65535² ; messages JSON-RPC mal formés ; 5000 abonnements ; ligne stdin de 70 Mio et ligne non UTF-8 |

## Défauts trouvés et corrigés (chacun a un test qui échoue sans le correctif)

| # | Défaut | Correctif | Test |
|---|---|---|---|
| D1 | Client : `Snapshot`/`Applied` de `seq = u64::MAX` → `last_seq + 1` déborde (panique en debug, arithmétique fausse en release) | `limits::check_seq` (`MAX_SEQ = 2^53`), session fermée avec erreur typée | `sequence_numbers_at_the_edge_of_u64_do_not_overflow` |
| D2 | `AddPage` avec objets ou calques de même id, `SetLayers` avec calques en double : accepté par l'hôte, document ambigu (l'écart hôte/copie autoritaire ne se résorbe jamais) | `check_ops` : identifiants uniques par page | `a_page_cannot_arrive_with_an_identifier_used_twice` |
| D3 | Client : un `Snapshot` avec deux pages de même id était accepté | `check_snapshot_shape` : ids de pages et de calques uniques | idem |
| D4 | Client : l'assembleur de parties de snapshot ne bornait pas les octets (4096 parties × 1 Mio) | `MAX_ASSEMBLY_BYTES` (256 Mio) | `a_guest_does_not_keep_more_snapshot_bytes_than_its_budget` |
| D5 | `inkboard-formats` : XML imbriqué de 100 000 balises → **débordement de pile** (abandon du processus) à l'ouverture d'un `.drawio` | profondeur 256 | `a_deeply_nested_drawio_file_is_an_error_not_a_stack_overflow` |
| D6 | draw.io compressé : **bombe de décompression** (`read_to_string` sans limite) | 64 Mio inflatés au plus | `a_compressed_diagram_that_inflates_to_gigabytes_is_refused` |
| D7 | SVG d'un invité : un `<path>` de 3,5 Mio (400 000 segments) se rastérise en 12 s (debug) sans aucun filtre | `check_untrusted_svg` : 1 Mio de géométrie (`d`, `points`) au plus | `an_svg_with_enormous_geometry_is_refused_to_guests` |
| D8 | Mermaid : mise en page plus que linéaire (400 nœuds / 4000 liens : 17 s en debug ; 3000 nœuds : 137 s) via `import_mermaid` ou l'ouverture d'un fichier | 1000 nœuds et 2500 liens au plus, refus avant la mise en page | `a_flowchart_too_large_to_lay_out_is_refused_before_the_layout` |
| D9 | MCP : `x = 1e300`, `w = -1.7e308`, `font_size` 1e308, noms de 30 000 caractères, formule de 200 000 caractères, objets « sous le contenu » au-delà de 1e7, `auto_layout`/`move_objects` qui dérivent : le tableau était enregistré (JSON écrit `null` pour un nombre non fini : **fichier illisible**) et, partagé, **chaque invité fermait la session** (limites de validation de l'hôte-de-confiance dépassées) | `check_args` (nombres, texte total 200 000 caractères, noms 200, profondeur), contrôle positions/tailles après chaque outil avec annulation (`try_transact`), formule 5000 caractères, colonne de tableau ≤ 2000, origine bornée | `extreme_numbers_are_refused_not_stored`, `spoiled_arguments_never_panic_or_ruin_the_board` |
| D10 | MCP : abonnements aux ressources sans limite (le guetteur interroge chacun toutes les 600 ms) | 256 abonnements, URI ≤ 4096 | `protocol_level_garbage_gets_an_error_or_nothing` |
| D11 | MCP : une ligne stdin non UTF-8 **arrêtait le serveur** ; une ligne de plusieurs Gio était gardée en mémoire | lecture bornée à 64 Mio (réponse `-32600`), UTF-8 remplacé | `garbage_lines_do_not_end_the_server_…` |

Aucun défaut de l'hôte lui-même (`session_host.rs`) n'a été trouvé : il n'a pas été modifié. Les bancs d'attaque sont des tests de non-régression de ce qu'ont posé SF2/S4 (limiteurs avant décodage, budgets d'octets, ordre des contrôles).

## Résultats des bancs longs (exécutés pendant ce lot)
- Messages : 30 000 cas de chaque (graines 777 puis 21/22) sans échec, après correctifs.
- Foule aléatoire contre l'hôte : 12 graines × 1500 ticks (graine de départ 5000), sans échec ni exclusion d'un invité honnête (une première version en excluait un : le harnais resynchronisait toutes les 500 ms, au-dessus de la limite de 1/s, c'est le harnais qui avait tort).
- MCP : 6 000 appels abîmés (graine 31337) sans échec après correctifs ; 8 000 avaient fait remonter D9 (formule, origine, dérive).

## Commandes
```
cargo +1.99.1 test -p inkboard-session --test hostile
cargo +1.99.1 test -p inkboard-app tests_hostile_host
cargo +1.99.1 test -p inkboard-formats --test hostile_files
cargo +1.99.1 test -p inkboard-mcp --test hostile --test stdin_limits

# bancs longs (comptes et graine ajustables ; défauts entre parenthèses)
INKBOARD_FUZZ_CASES=150000 INKBOARD_FUZZ_SEED=1 cargo +1.99.1 test -p inkboard-session --test hostile -- --ignored        # (150 000 / 300 000 / 200 000 / 300 000 cas)
INKBOARD_FUZZ_CASES=40   INKBOARD_FUZZ_SEED=1000 cargo +1.99.1 test -p inkboard-app tests_hostile_host::long -- --ignored   # graines x 1500 ticks (40)
INKBOARD_FUZZ_CASES=20000 INKBOARD_FUZZ_SEED=900 cargo +1.99.1 test -p inkboard-mcp --test hostile -- --ignored             # appels (20 000)
INKBOARD_MEMORY_TICKS=600 cargo +1.99.1 test -p inkboard-app tests_hostile_host::memory                                        # mémoire : vague plus longue (24)
```
`PROPTEST_RNG_SEED=<n>` change la graine des tests `inkboard-formats` (proptest). Une exécution longue de l'hôte prend plusieurs minutes en debug (≈ 16 s par graine) ; utiliser `--release` pour aller plus vite. Pour suivre l'appel qui bloque dans le banc MCP : `INKBOARD_FUZZ_TRACE=1 … -- --nocapture`.

## Ouvert (non corrigé)
- **Octets du tableau de l'hôte** : seul le *nombre* d'objets est plafonné (50 000). Un éditeur peut faire grossir le document à ~8 Mio/s (limite d'octets reçus) jusqu'à ce que l'hôte l'exclue ou l'arrête : pas de plafond d'octets du tableau, ni de l'historique propre de l'hôte.
- **Objets de l'hôte lui-même** (interface, collage, import) au-delà des limites `host_trust` (tableau > 200 colonnes ou lignes, texte > 1 M caractères, police > 2000) : les invités refusent le tableau et quittent. Seul MCP est maintenant borné par ces limites ; l'interface ne l'est pas.
- Parties d'images d'invité en cours : 2 envois × 15 Mio par invité, conservés pendant le maintien de place (60 s) : borné (≈ 300 Mio au pire pour 10 invités) mais élevé.
- Rendu d'un SVG envoyé par un **hôte** hostile à un invité (S-09, déjà ouvert) : `<filter>` lourd sans borne de temps ; la géométrie de 1 Mio n'est contrôlée que côté hôte pour les images d'invités. Coût de mise en page de texte très long (100 000 caractères dans un objet) non mesuré.
- Rejeu : un relais qui rejoue la première trame d'un invité (`Hello` avec preuve, sans défi) crée une demande de consentement au nom de l'invité légitime. Il ne lit ni n'écrit rien (pas de clé) mais l'hôte voit une demande en double ; l'identité de transport du rejeu est différente. Non corrigé (nécessiterait un défi/nonce dans le protocole).
- `cargo-fuzz` : non tenté (voir Méthode). Le chemin réseau réel (`iroh`) n'a pas été attaqué : tout passe par le SimNet, qui n'impose que la limite de trame (1 Mio).
- Dérive résiduelle : `check positions` après un outil MCP protège les objets avec cadre ; les points de crayon (non créables par MCP) et les points `via` des connecteurs ne sont pas relus.
