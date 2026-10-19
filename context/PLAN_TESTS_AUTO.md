# Plan de tests automatisés — Inkboard

> Destinataire : agent d'implémentation (Sonnet, palier T1). Rédigé le 2026-10-19.
> Objectif : couvrir sans humain ~80 % de l'application. Ce qui reste humain est listé à la fin.

## 0. Règles d'exécution (à respecter strictement)

1. **Un lot à la fois**, dans l'ordre du § 3. À la fin d'un lot : lancer les commandes de contrôle, remplir le
   **Journal** (§ 6), puis **s'arrêter** et attendre la validation. Ne pas enchaîner le lot suivant.
2. **Ne lire que** ce fichier et les fichiers listés dans « Fichiers » du lot en cours. Pas d'exploration libre du dépôt.
3. **Ne pas modifier le code produit.** Seules exceptions autorisées :
   - rendre visible un élément pour les tests (`pub(crate)`, `pub(super)`) ;
   - déclarer un module de test (`#[cfg(test)] mod …;`) ;
   - ajouter des `[dev-dependencies]`.
4. **Un test qui révèle un bug n'est pas corrigé dans le lot.** Garder le test, le marquer
   `#[ignore = "BUG-<n> : <résumé>"]`, décrire le bug dans le Journal (entrée, sortie attendue, sortie obtenue), continuer le lot.
5. **Contrôles obligatoires avant de rendre la main** (les trois doivent passer) :
   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Budget de temps** : `cargo test --workspace` ne doit pas prendre plus de ~60 s de plus qu'avant le plan. Ce qui est lent
   ou dépend d'un outil externe va derrière `#[ignore]` ou dans un job CI dédié.
7. **Pas de réseau dans `cargo test`.** Les téléchargements se font seulement dans des scripts ou jobs CI, avec une URL
   épinglée (commit SHA ou version exacte) et un contrôle SHA-256.
8. **Dépendances** : uniquement en `[dev-dependencies]`, version épinglée (`"x.y.z"`), licence MIT, Apache-2.0, BSD,
   ISC ou Zlib. Rust stable uniquement (MSRV du workspace : 1.90).
9. **CI** : nouveaux jobs dans de **nouveaux** fichiers de workflow ; ne pas toucher à `ci.yml` sauf mention explicite dans le
   lot, ni à la logique de publication de `release.yml`. Le quota d'artefacts est déjà atteint : aucun `upload-artifact`
   sauf `if: failure()` avec `retention-days: 3`.
10. **Interdit sans accord explicite de l'utilisateur** : `git push`, créer un tag, `gh workflow run`, tout appel payant
    (API Claude, etc.), commit. Préparer, puis demander.
11. Style : celui du code voisin (commentaires courts en anglais, noms explicites). Pas de refactor non demandé.

## 1. État des lieux (vérifié le 2026-10-19)

- 265 tests ; CI `.github/workflows/ci.yml` : fmt + clippy `-D warnings` + `cargo test` sur Windows, Linux, macOS.
- `inkboard-app` : 84 tests d'interface sans fenêtre avec `egui_kittest` 0.33.3 (`src/tests.rs`, helper `struct T` :
  `tool`, `click`, `drag`, `key`, `key_mod`, `objects`, `add_rect`…).
- `inkboard-formats` : `import_any(file_name, content, &dyn Measurer)` et `detect()` dans `src/lib.rs` ;
  `tests/realistic.rs` (fichiers écrits à la main « comme les vrais outils »). Aucun vrai fichier d'utilisateur.
- `inkboard-mcp` : `tests/session.rs` lance le vrai binaire (`struct Client`, `--root`, `--no-live`) ; confinement des chemins
  dans `src/tools.rs` → `fn resolve` (canonicalise **le dossier parent seulement**).
- `inkboard-core` : `serial::save` (`src/serial.rs:102`) écrit un fichier temporaire puis `rename` (atomique).
- App : options `--screenshot <png>`, `--ui-demo`, `--bench`, `--bench-close`, `--pen-test`, `--edit-demo`,
  `--formula-demo`, `--shortcuts-demo`. CLI : `ink convert`, `ink info`, `ink bench N`.
- Les `unwrap()` de `mermaid.rs` hors tests initialisent tous des `Regex` statiques : pas un risque de plantage.
- Aucun `proptest`, `cargo-deny`, snapshot visuel, test d'installeur, test sous Linux/macOS avec fenêtre réelle.

## 2. Carte de couverture

| Risque non vérifié aujourd'hui | Lot |
|---|---|
| Fichiers réels draw.io / Excalidraw / Canvas / Mermaid | A |
| Plantage sur fichier malformé | A, G |
| Agent MCP : confinement, erreurs, conformité protocole | B |
| Application jamais lancée sous Linux / macOS | C |
| Invariants du moteur (annuler, routage, index, sérialisation) | D |
| Perte de données (enregistrement interrompu, fichier corrompu) | E |
| Régression de performance | E |
| Commandes et outils de l'interface, traductions | F |
| Licences et vulnérabilités des dépendances | G |
| Régression visuelle (rendu, thèmes, langues, exports) | H |
| Installeurs jamais exécutés | I |
| Stylet (pression, gomme) jamais validé | J |

## 3. Lots

### Lot A — Formats : vrais fichiers, aller-retour, entrées aléatoires

**But** : prouver que l'import ne plante jamais et ne perd rien en silence, et que nos exports sont relus à l'identique.

**Fichiers** : `crates/inkboard-formats/src/lib.rs` (lecture), les modules `drawio.rs`, `excalidraw.rs`, `jsoncanvas.rs`,
`mermaid.rs` (lecture des fonctions publiques seulement), `crates/inkboard-formats/Cargo.toml`,
nouveaux : `tests/corpus.rs`, `tests/roundtrip.rs`, `tests/random_input.rs`, `tests/corpus/**`.

**Étapes**
1. **Corpus** dans `crates/inkboard-formats/tests/corpus/<format>/` : 5 à 15 fichiers réels par format.
   Ne committer que des fichiers sous licence MIT, Apache-2.0 ou CC0 :
   - Mermaid : exemples de la documentation de `mermaid-js/mermaid` (MIT), pour `flowchart`, `stateDiagram`,
     `sequenceDiagram`, plus 2 types non gérés (`gantt`, `pie`), qui doivent renvoyer `Err` ;
   - draw.io, Excalidraw, Obsidian Canvas : dépôts GitHub publics sous licence compatible.
   Tracer chaque fichier dans `tests/corpus/SOURCES.md` (URL épinglée au commit, licence, SHA-256). Si une licence est
   incompatible ou incertaine, ne pas committer : le noter dans le Journal.
2. `tests/corpus.rs` : pour chaque fichier, `import_any` est appelé dans `std::panic::catch_unwind`.
   - Il ne doit **jamais** paniquer.
   - Pour un format géré : `Ok`, au moins 1 objet, et autant d'objets texte que de libellés non vides du fichier source,
     comptés par une lecture simple du fichier (balises `value=` pour draw.io, `type == "text"` pour Excalidraw, etc.).
   - Pour un type non géré : `Err` avec un message non vide.
3. `tests/roundtrip.rs` : construire un document de référence couvrant les types d'objets (rectangle, ellipse, texte,
   connecteur lié, tableau, bloc de code, image). Pour chaque format exportable (draw.io, Excalidraw, JSON Canvas) :
   export → import → comparer.
   - Doivent être égaux : le nombre d'objets par type, les textes, et les liaisons des connecteurs (source/cible).
   - Positions et tailles : tolérance de 1 px.
   - Exclure les pertes documentées (traits au crayon exportés en image vers draw.io ; JSON Canvas : formes avec texte et
     flèches seulement) avec un commentaire qui renvoie à `docs/ROADMAP.md`.
4. `tests/random_input.rs` avec `proptest` (dev-dependency) : chaînes aléatoires, puis mutations des fichiers du corpus
   (troncature à une position aléatoire, inversion d'octets, suppression d'une ligne) → `import_any` ne panique jamais.
   256 cas par propriété maximum.
5. *(Optionnel, job CI séparé `.github/workflows/formats-external.yml`, Linux, `workflow_dispatch` + hebdomadaire)* :
   installer draw.io desktop (version épinglée, `.deb` des releases `jgraph/drawio-desktop`) et exporter en PNG, sous
   `xvfb-run`, chaque `.drawio` produit par nos exports. Le job réussit si le code de sortie vaut 0 et le PNG n'est pas vide.
   Pour Excalidraw : spike limité à 1 h ; abandonner (et le noter) s'il faut un vrai navigateur.

**Critère de réussite** : les trois fichiers de test passent, aucun panic. Les éventuels bugs sont en `#[ignore = "BUG-n"]`
et décrits dans le Journal.

### Lot B — Serveur MCP : outils, erreurs, sécurité

**Fichiers** : `crates/inkboard-mcp/tests/session.rs`, `crates/inkboard-mcp/src/tools.rs` (lecture : liste des outils, `resolve`),
`docs/MCP.md` (lecture), nouveaux : `tests/common/mod.rs`, `tests/tools_all.rs`, `tests/security.rs`, `scripts/mcp_e2e_claude.ps1`.

**Étapes**
1. Déplacer `struct Client` de `session.rs` vers `tests/common/mod.rs` et l'utiliser depuis les trois fichiers.
   C'est le seul refactor autorisé dans ce lot.
2. `tests/tools_all.rs` : `tools/list` renvoie les 16 outils documentés. Pour **chaque** outil :
   - un appel valide → résultat sans erreur, et le fichier sur disque reflète la modification (relu avec `inkboard_core::serial`) ;
   - un appel aux arguments invalides (champ manquant, mauvais type) → erreur JSON-RPC, et le serveur répond encore à
     l'appel suivant.
   Ajouter aussi un message JSON malformé sur stdin : le serveur ne s'arrête pas.
3. `tests/security.rs`, avec un `--root` temporaire :
   - `../evil.inkboard.json` → refusé ;
   - un chemin absolu hors de la racine → refusé ;
   - un dossier lien symbolique pointant hors de la racine → refusé ;
   - **un fichier final qui est un lien symbolique pointant hors de la racine** → doit être refusé.
     `resolve` ne canonicalise que le parent : ce cas est suspect. S'il échoue, c'est BUG-n, à ne pas corriger.
   Sous Windows, les liens symboliques peuvent exiger un privilège : si leur création échoue, sauter le test avec un message
   (pas un échec).
4. *(Optionnel, job CI `mcp-conformance.yml`, Linux, Node LTS)* : `npx @modelcontextprotocol/inspector@<version épinglée> --cli`
   sur le binaire `inkboard-mcp` : `tools/list`, `resources/list`, un `tools/call`.
5. `scripts/mcp_e2e_claude.ps1` : **écrire le script, ne pas l'exécuter** (appel payant). Le script :
   - lance l'application sur un tableau temporaire ;
   - lance `claude -p` avec une configuration MCP pointant sur `inkboard-mcp` et une consigne fixe
     (« crée 3 formes reliées par des connecteurs ») ;
   - vérifie le fichier obtenu avec `ink info`.
   Usage documenté en tête du script.

**Critère de réussite** : tests 2 et 3 verts (ou bugs en `#[ignore]` documentés) ; script 5 présent et relu.

### Lot C — Lancer la vraie application sur les 3 systèmes (CI)

**Fichiers** : `crates/inkboard-app/src/main.rs` et `src/app.rs` (lecture : comportement de `--screenshot`),
nouveaux : `.github/workflows/smoke.yml`, `scripts/check_png.py`.

**Étapes**
1. En local sous Windows, vérifier que `cargo run --release -p inkboard-app -- --screenshot out/win.png` produit le PNG et
   **quitte seul**. Si l'application ne quitte pas, s'arrêter et le signaler (ce lot en dépend).
2. `scripts/check_png.py <png>` (bibliothèque standard uniquement : `zlib` + `struct`) : vérifie que le PNG fait au moins
   800×600 et n'est pas uniforme (au moins 50 couleurs distinctes sur un échantillon). Code de sortie 1 sinon.
3. `smoke.yml` (`push` sur `main` + `workflow_dispatch`), matrice windows-latest / ubuntu-latest / macos-latest,
   `timeout-minutes: 15`. Pour chaque système :
   - build release ;
   - `--screenshot`, `--ui-demo menu` et l'ouverture d'un fichier du corpus du lot A, chacune avec capture puis `check_png.py`.
   Spécificités :
   - **Linux** : `sudo apt-get install -y xvfb mesa-utils libgl1-mesa-dri libxkbcommon-x11-0`, puis `xvfb-run -a` et
     `LIBGL_ALWAYS_SOFTWARE=1` ;
   - **macOS** : lancement direct (le runner a une session graphique) ;
   - toutes les commandes sous `timeout 120` (ou l'équivalent PowerShell sous Windows) ; un blocage = échec.
   Envoi des PNG seulement `if: failure()`.
4. Tester aussi `INKBOARD_RENDERER=wgpu` et `INKBOARD_RENDERER=glow` (2 lignes de matrice supplémentaires sous Linux).

**Critère de réussite** : workflow vert sur les 3 systèmes. Comme le lancement exige un push, préparer et demander à
l'utilisateur ; sans push, se limiter à valider l'étape 1 et `actionlint` si disponible.

### Lot D — Moteur : tests de propriétés

**Fichiers** : `crates/inkboard-core/Cargo.toml`, signatures publiques de `src/editor.rs`, `src/serial.rs`, `src/connector.rs`,
`src/route.rs`, `src/spatial.rs`, `src/transform.rs` (lecture), nouveau : `crates/inkboard-core/tests/properties.rs`.

**Propriétés** (`proptest`, 256 cas maximum, générateur de documents aléatoires de 0 à 200 objets) :
1. Pour toute suite d'opérations d'édition : N × annuler revient au document initial ; N × rétablir revient au document final.
2. `serial` : sauvegarder puis recharger donne un document égal (sauvegarde dans un `tempdir`).
3. Connecteurs liés : après déplacement aléatoire des formes, les extrémités restent sur le contour de leur forme
   (tolérance 0,5 px).
4. Routage coudé avec contournement : aucun segment ne traverse l'intérieur d'une forme non reliée (sous la limite de
   5 000 objets).
5. Index spatial : une requête rectangulaire renvoie le même ensemble d'objets qu'une recherche exhaustive.
6. Transformations : appliquer puis inverser revient au cadre d'origine (tolérance 1e-6).

**Critère de réussite** : propriétés vertes, ou bugs en `#[ignore]` avec le cas minimal produit par proptest copié dans le Journal.

### Lot E — Robustesse des données et garde-fou de performance

**Fichiers** : `crates/inkboard-core/src/serial.rs` (lecture), `crates/inkboard-app/src/app.rs` (lecture : `save*`, restauration
de la sauvegarde automatique vers la ligne 266), `crates/inkboard-cli/src/*.rs` (lecture : `bench`), nouveaux :
`crates/inkboard-core/tests/durability.rs`, `crates/inkboard-cli/tests/kill_during_save.rs`, `.github/workflows/bench.yml`.

**Étapes**
1. `durability.rs` :
   - un fichier natif tronqué, de mauvaise version ou avec des champs inconnus → `Err` lisible, pas de panic ;
   - un enregistrement dans un dossier en lecture seule → `Err`, fichier existant intact ;
   - un fichier temporaire laissé par un enregistrement interrompu → le fichier cible reste lisible.
2. `kill_during_save.rs` : lancer `ink convert` (via `CARGO_BIN_EXE_ink`) sur un gros document généré, le tuer à des
   instants aléatoires (10 essais). La cible doit toujours être soit absente, soit l'ancienne version, soit la nouvelle
   version complète et valide. Jamais un fichier à moitié écrit.
3. Test `egui_kittest` (ajouté au fichier du lot F, ou ici s'il passe avant) : ouvrir un fichier corrompu dans l'application
   affiche une erreur et ne vide pas le tableau courant.
4. `bench.yml` (`workflow_dispatch` + hebdomadaire, ubuntu-latest) : `ink bench 20000` × 5, médiane comparée à un seuil
   égal à 3 × la valeur mesurée sur le runner lors de la création du job (écrire la mesure de référence dans le
   workflow, en commentaire). Échec au-delà.

**Critère de réussite** : tests 1 et 2 verts ; le job de performance est prêt (son lancement demande un push).

### Lot F — Interface : toutes les commandes, tous les outils, traductions

**Fichiers** : `crates/inkboard-app/src/tests.rs` (lecture du helper `T`, rendre `T` et ses méthodes `pub(super)`),
`src/keymap.rs` (lecture : liste des actions), `src/tools.rs` (lecture : `Tool`, `tool_lock`), `src/i18n.rs`, `src/i18n_fr.rs`
(lecture), la déclaration des modules (`main.rs` ou `lib.rs`), nouveau : `crates/inkboard-app/src/tests_ui.rs`.

**Étapes**
1. Avant d'écrire : lister ce que les 84 tests existants couvrent déjà, et ne pas dupliquer (le noter dans le Journal).
2. **Chaque action** du keymap (41) : déclencher par son raccourci par défaut, sur un tableau avec une sélection
   représentative. Pas de panic, et un effet observable quand il est simple à vérifier (objet ajouté, retiré, déplacé,
   zoom changé…).
3. **Chaque outil** : créer un objet, puis vérifier qu'on revient à `Tool::Select` ; refaire avec le verrou (`Q`) :
   l'outil reste actif et un 2e clic crée un 2e objet. Cas Texte : après création, l'objet est en édition ; avec le verrou,
   un clic ailleurs termine l'édition sans créer de zone vide.
4. **Traductions** :
   - chaque clé anglaise a une traduction française non vide (si `i18n.rs` le teste déjà, ne pas dupliquer) ;
   - en français, fenêtre de 1024×700 : aucun bouton de la barre d'outils ni du menu ne sort de l'écran ni n'en
     chevauche un autre (rectangles obtenus via l'arbre AccessKit du `Harness`).
5. Raccourcis personnalisés : un `keymap.json` invalide est ignoré, l'erreur est signalée, les valeurs par défaut s'appliquent.

**Critère de réussite** : tous verts ; la durée totale des tests de l'app augmente de moins de 30 s.

### Lot G — Dépendances : licences et vulnérabilités

**Fichiers** : nouveau `deny.toml` ; `.github/workflows/ci.yml` (autorisé : **ajout d'un job seulement**) ;
`THIRD_PARTY_LICENSES.md` (lecture).

**Étapes**
1. `deny.toml`. Licences autorisées : MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, OFL-1.1 (polices, dont
   STIX Two Math), MPL-2.0 si déjà présente. Advisories : `deny` ; doublons de versions : `warn`.
2. Job `deny` dans `ci.yml` avec `EmbarkStudios/cargo-deny-action@v2`, Linux seulement.
3. En local : `cargo deny check`. Lister dans le Journal toute licence ou alerte, sans changer de dépendance.

**Critère de réussite** : `cargo deny check` vert, ou écarts listés pour décision de l'utilisateur.

### Lot H — Régression visuelle

**Fichiers** : `crates/inkboard-app/Cargo.toml` (activer les fonctions `snapshot` et `wgpu` d'`egui_kittest`),
`src/tests.rs` (helper), `crates/inkboard-formats/src/raster.rs` (lecture), nouveaux : `src/tests_visual.rs`,
`crates/inkboard-app/tests/snapshots/`.

**Étapes**
1. 10 captures maximum, fenêtre de 1024×700 :
   - tableau vide en thème clair et en thème sombre ;
   - menu principal ouvert ;
   - panneau de style ;
   - un tableau de démonstration avec tous les types d'objets, en anglais et en français ;
   - un tableau, un bloc de code, une formule.
2. Exports : le tableau de démonstration exporté en SVG puis rendu en PNG (même moteur que `raster.rs`), et en PNG direct.
   Comparer aux références.
3. Le rendu varie selon le GPU : ces tests ne tournent **que si** `INKBOARD_SNAPSHOTS=1`, avec un seuil de tolérance, et
   dans un seul job CI (Linux, rendu logiciel). Les références sont produites par ce job : documenter dans le fichier
   comment les régénérer (`UPDATE_SNAPSHOTS=1`).

**Critère de réussite** : tests verts dans le job dédié ; dépôt alourdi de moins de 2 Mo.

### Lot I — Installeurs

**Fichiers** : `.github/workflows/release.yml` (autorisé : **ajouter des étapes de vérification après la construction**,
sans toucher à la publication), `Packager.toml` (lecture), `docs/DISTRIBUTION.md` (lecture).

**Étapes** (avec `workflow_dispatch`, qui construit sans publier) :
1. **Windows NSIS** : `setup.exe /S`, retrouver le dossier d'installation **dans le journal ou le registre** (ne pas le
   supposer), lancer `inkboard --screenshot` puis `check_png.py`, désinstaller en silencieux, vérifier que le dossier
   a disparu.
2. **Windows MSI** : `msiexec /i … /qn /l*v install.log`, mêmes vérifications, puis `msiexec /x … /qn`.
3. **Linux** : `sudo apt-get install -y ./inkboard_*.deb`, lancement sous `xvfb-run` ; l'AppImage avec
   `--appimage-extract-and-run`.
4. **macOS** : `hdiutil attach`, copie du `.app`, lancement du binaire avec `--screenshot`, `hdiutil detach`.
5. Ne **pas** lancer le workflow, ne **pas** créer le tag `v0.1.0-rc1` : préparer, puis demander à l'utilisateur.

**Critère de réussite** : workflow syntaxiquement valide (`actionlint` si disponible) et relu ; lancement par l'utilisateur.

### Lot J — Stylet sans matériel (Windows)

**Fichiers** : `crates/inkboard-pen/Cargo.toml`, `src/win.rs` (lecture), `docs/STYLET.md` (lecture : rapport de `--pen-test`),
nouveau : `crates/inkboard-pen/tests/synthetic.rs`.

**Étapes**
1. Vérifier dans `Cargo.toml` comment `inkboard-pen` autorise `unsafe` malgré le `forbid` du workspace ; le test hérite de
   la même règle. `unsafe` est **interdit** dans tout autre crate.
2. Test Windows seulement, `#[ignore]` (il faut une session de bureau) :
   - lancer l'application avec `--pen-test` ;
   - créer un stylet virtuel (`CreateSyntheticPointerDevice`, `PT_PEN`) ;
   - injecter avec `InjectSyntheticPointerInput` un trait à pression 0,2, puis un à 1,0, puis un passage avec le bout gomme
     (`PEN_FLAG_ERASER` / `PEN_FLAG_INVERTED`) ;
   - lire le rapport produit et vérifier : largeurs différentes, et le trait effacé par la gomme.
   Ajouter les fonctionnalités `windows-sys` nécessaires en `[target.'cfg(windows)'.dev-dependencies]`.
3. Lancement : `cargo test -p inkboard-pen -- --ignored`, en local. Le runner Windows de GitHub a une session de bureau :
   proposer (sans l'ajouter) un job dédié.

**Limite à noter dans le Journal** : ce test valide le chemin `WM_POINTER` de l'application, pas les pilotes réels
(courbes de pression Wacom, N-trig, etc.).

## 4. Ce qui reste humain (~15–20 %)

- Ergonomie et sensation d'utilisation (fluidité ressentie, gestes, découvrabilité, par exemple du cadenas d'outil).
- Jugement visuel (une référence peut être « stable » et laide).
- Vrais stylets et pilotes ; écrans HiDPI exotiques et plusieurs écrans.
- Fichiers privés des utilisateurs ; lecteurs d'écran.
- MCP avec de vrais clients au quotidien (le lot B ne fait qu'un scénario fixe).

## 5. Hors périmètre (décisions séparées, ne pas implémenter ici)

- Passer à l'extension `.inkboard` et à l'association de fichier dans l'installeur. Le lot I pourra ensuite vérifier
  l'association.
- Rendre le verrou d'outil plus visible (info-bulle, double-clic sur l'outil pour verrouiller).

## 6. Journal (à remplir par l'agent, un bloc par lot)

```
### Lot X — <date>
- Fait :
- Fichiers créés / modifiés :
- Commandes et résultats (durée de cargo test avant/après) :
- Bugs trouvés (BUG-n : entrée → attendu / obtenu, test concerné) :
- Écarts au plan et pourquoi :
- Reste à faire / besoin d'une décision :
```

### Lot A — 2026-10-19
- Fait : corpus de 45 vrais fichiers (mermaid 15 dont 2 gantt + 2 pie qui donnent `Err`, draw.io 8 dont 3 compressés, Excalidraw 11, JSON Canvas 11),
  tracés dans `SOURCES.md` (URL épinglée au commit, licence, SHA-256). `corpus.rs` (aucun panic, `Ok`, ≥ 1 objet, libellés), `roundtrip.rs`
  (draw.io, Excalidraw, JSON Canvas : types, textes, liaisons, positions à 1 px), `random_input.rs` (proptest, 256 cas par propriété).
  Aucune modification du code produit. Téléchargement du corpus fait une fois à la main (`gh api` pour le SHA, `curl` sur `raw.githubusercontent.com/<repo>/<sha>/…`) ;
  aucun accès réseau dans les tests.
- Fichiers créés / modifiés :
  - `crates/inkboard-formats/Cargo.toml` : `[dev-dependencies] proptest = { version = "1.12.0", default-features = false, features = ["std"] }` (MIT OR Apache-2.0, MSRV 1.88).
  - `Cargo.lock` : +8 paquets (proptest 1.12.0, rand 0.10.3, rand_core, rand_xorshift, chacha20, cpufeatures, core_detect, unarray). Cargo a dû télécharger ces crates.
  - `crates/inkboard-formats/tests/{corpus.rs, roundtrip.rs, random_input.rs}`.
  - `crates/inkboard-formats/tests/corpus/{mermaid,drawio,excalidraw,canvas}/*`, `corpus/SOURCES.md`, `corpus/.gitattributes` (`* -text` : le dépôt force `eol=lf`, ce qui changerait les octets et donc les SHA-256).
- Commandes et résultats :
  - `cargo fmt --all --check` : OK.
  - `cargo clippy --workspace --all-targets -- -D warnings` : OK (0 avertissement).
  - `cargo test --workspace` : OK, 291 réussis, 8 ignorés (avant : 276 réussis, 2 ignorés). Durée 21,8 s avant, 25,0 s après (cache chaud ; la première
    compilation de proptest ajoute ~10 s une seule fois). Les 3 fichiers de test durent 0,6 s, 0,1 s et 0,0 s.
  - `cargo test -p inkboard-formats -- --include-ignored` : les 6 tests ignorés échouent, tous pour les bugs ci-dessous.
- Bugs trouvés :
  - **BUG-1 — Excalidraw : un texte lié à une flèche est importé deux fois.**
    Entrée : une flèche `{"id":"a1","type":"arrow",…,"boundElements":[{"id":"t1","type":"text"}]}` et `{"id":"t1","type":"text","containerId":"a1","text":"yes",…}`.
    Attendu : un seul libellé (celui du connecteur). Obtenu : `Connector.label = "yes"` et en plus un objet texte libre « yes » (`excalidraw.rs`, branche `"text"` :
    seuls les conteneurs rectangle/ellipse/losange font sauter le texte). Touche aussi notre propre export (aller-retour d'un connecteur avec libellé) et 5 fichiers réels
    (abdheshnayak, aurorascharff, parmsam, renatus-cartesius, swiftlysingh : +2 à +12 textes en trop).
    Tests : `corpus::bug_1_arrow_label_is_imported_twice`, `roundtrip::excalidraw_connector_label_is_read_back_once` (`#[ignore]`).
  - **BUG-2 — un connecteur lié à un tableau perd son lien à l'aller-retour (draw.io et Excalidraw).**
    Entrée : rectangle « A », tableau 2×2, connecteur lié A → tableau, export puis import.
    Attendu : un connecteur lié aux deux bouts. Obtenu : draw.io, le connecteur disparaît (`report.skipped = 1`) car l'export écrit `target="tb-<id du tableau>"`
    alors que les cellules s'appellent `tb-<id>-<r>-<c>` ; Excalidraw, le connecteur reste mais un bout est libre (`endBinding.elementId` = id du tableau, qui n'est pas un élément).
    (Un connecteur lié à une image survit dans les deux formats : testé, vert.)
    Tests : `roundtrip::{drawio,excalidraw}_keeps_a_connector_bound_to_a_table` (`#[ignore]`).
  - **BUG-3 — Excalidraw : un élément sans `"id"` fait paniquer l'import.**
    Entrée minimale : `{"type":"excalidraw","elements":[{"type":"rectangle","x":0,"y":0,"width":10,"height":10}]}` (trouvée par proptest en supprimant une ligne `"id"`
    d'un vrai fichier). Attendu : `Err` ou élément ignoré. Obtenu : panic `no entry found for key` (`excalidraw.rs`, `ids[id]` aux lignes ~403, 419, 475, 505, 520).
    Tests : `random_input::bug_3_excalidraw_element_without_id`, `random_input::bug_3_excalidraw_without_one_line_never_panics` (`#[ignore]`).
  - Autre chasse : 30 000 cas par propriété (réglage temporaire, remis à 256) sur mermaid, draw.io et canvas, sans aucun autre panic.
- Observation (pas un bug testé) : draw.io, une forme `image=img/lib/…` (icône de la bibliothèque intégrée, pas une image incorporée) est abandonnée avec son libellé ;
  c'est compté dans `report.skipped` (ex. `KamalRathnayake__17.drawio` : 5 libellés d'icônes perdus, tous couverts par `skipped`). Le test tolère une perte tant qu'elle est
  signalée (chaque élément ignoré excuse au plus un libellé). Décision à prendre : garder un rectangle avec le libellé à la place ?
- Écarts au plan et pourquoi :
  - Mermaid : pas de comptage de libellés (aucune lecture simple fiable de la syntaxe) ; le test exige `Ok`, ≥ 1 objet et ≥ 1 objet avec texte.
  - Libellés : le test exige « pas plus que la source, et un manque seulement s'il est signalé dans `report.skipped` » ; JSON Canvas exige en plus `skipped` exact.
    Excalidraw : les cadres nommés comptent comme un libellé (l'import crée un titre).
  - Corpus draw.io : 8 fichiers (un diagramme vide, valide mais à 0 objet, a été retiré ; 3 fichiers > 100 Ko écartés). Les modèles de `jgraph/drawio` sont sous CC-BY 4.0
    (licence hors liste) et `drawio-desktop` est sous GPL-3.0 : non utilisés. Licences = celles du dépôt selon GitHub (SPDX) ; la provenance de chaque fichier n'est pas auditée.
  - Fichiers Mermaid : blocs de code extraits tels quels de `packages/mermaid/src/docs/syntax/*.md` (commit `81ad807…`), nommés `<page>_<numéro de bloc>.mmd`.
  - Aller-retour : un tableau revient en N rectangles (draw.io, Excalidraw) ; draw.io recentre le contenu (décalage commun toléré) ; JSON Canvas : tout devient un rectangle
    (cadre arrondi, minimum 60×40, angle perdu), tableau et image comptés comme abandonnés (`dropped == 2`). Pas de traits au crayon dans le document de référence (perte documentée).
  - proptest : graine fixe (2026, `PROPTEST_RNG_SEED=<n>` pour explorer) pour que la CI reste reproductible tant que BUG-3 existe ; pas de fichier de régressions écrit dans le dépôt.
    Les fichiers Excalidraw sont exclus des mutations « actives » jusqu'à la correction de BUG-3 (le test ignoré les couvre).
  - Étape 5 (job CI draw.io externe, `formats-external.yml`) : optionnelle, non faite.
- Reste à faire / besoin d'une décision :
  - Corriger BUG-1, BUG-2, BUG-3 (lot de correction séparé) ; ensuite retirer les `#[ignore]`, `BUG_1_FILES` et ajouter `"excalidraw"` à `CORPUS` dans `random_input.rs`.
  - Décider du traitement des icônes draw.io de la bibliothèque intégrée (observation ci-dessus).
  - Valider le choix de la graine fixe, et le fait de se fier à la licence du dépôt (pas d'audit fichier par fichier) pour le corpus.

### Correctifs BUG-1..3 — 2026-10-19
- Fait :
  - BUG-3 (`excalidraw.rs`, `import`) : plus aucune indexation `ids[...]`. Un élément sans `"id"` reçoit un id synthétique (`own_id` : `ids.get(id)` sinon `Id::new()`), y compris pour le point d'arrivée d'une liaison. Aucun `unwrap`/index panicogène sur les données d'entrée dans ce module (les autres index suivent un contrôle de longueur).
  - BUG-1 (`excalidraw.rs`, `import`, branche `"text"`) : un texte dont le `containerId` désigne une flèche ou une ligne et qui est consommé comme étiquette est ignoré, comme pour rectangle/ellipse/losange.
  - BUG-2 (`excalidraw.rs`, `export`, `Kind::Table` ; `drawio.rs`, `page_xml`, `Kind::Table`) : si un connecteur de la page cible le tableau, l'export ajoute avant les cellules un rectangle englobant invisible (sans trait ni fond) portant l'id du tableau (`tb-<id>` pour draw.io, `<id>` pour Excalidraw). Ainsi le lien existant (`target="tb-<id>"`, `endBinding.elementId`) vise un vrai élément, sans toucher à l'import ni au format de lien.
  - Choix BUG-2 : plus simple et fidèle qu'un lien vers une cellule (qui accrocherait le coin haut-gauche) ; le connecteur s'accroche au contour du tableau, comme dans Inkboard. Coût : un tableau ciblé revient avec un rectangle de plus (transparent) ; un tableau non ciblé est exporté comme avant.
- Tests : `random_input.rs` (`"excalidraw"` ajouté à `CORPUS`, `#[ignore]` retirés), `corpus.rs` (`BUG_1_FILES` et `bug_1_arrow_label_is_imported_twice` supprimés ; `excalidraw_files_import_without_losing_labels` couvre les 11 fichiers), `roundtrip.rs` (3 `#[ignore]` retirés).
- Commandes et résultats : `cargo fmt --all --check` OK (un `cargo fmt --all` a reformaté une ligne de `excalidraw.rs`) ; `cargo clippy --workspace --all-targets -- -D warnings` OK ; `cargo test --workspace` OK, 296 réussis, 2 ignorés (aucun `BUG`) ; `grep -rn 'ignore = "BUG' crates` : aucun résultat.
- Écarts : aucun test d'un autre format cassé. Le test `bug_3_excalidraw_without_one_line_never_panics` et la static `EXCALIDRAW` sont maintenant redondants avec `CORPUS` (suppression possible, non faite). Un texte lié à une ligne rejetée (moins de 2 points) ou reconnue comme triangle est perdu (rare). Icônes draw.io `img/lib/…` : non traité, comme demandé.

### Lot B — 2026-10-19
- Fait : `struct Client` (et `temp_dir`) déplacés dans `tests/common/mod.rs` (+ `send_raw` pour une ligne brute ; `#![allow(dead_code)]` car chaque fichier de test compile le module seul). `session.rs` inchangé hors son en-tête (`mod common;`), 7 tests verts à l'identique.
  `tools_all.rs` (4 tests) : `tools/list` = les 16 outils de `docs/MCP.md` avec description et schéma ; un appel valide par outil avec relecture via `inkboard_core::serial` (nom, cadre, texte, connecteur lié des deux bouts, 4 cellules de tableau, bloc de code, formule `latex`, image + asset, `import_mermaid` = 3 objets, `update_object`, `auto_layout` déplace, `delete_object`, `add_page`) ; `describe_board` / `board_changes` / `save_board` laissent le fichier identique octet pour octet ; `export` SVG contient le texte modifié ; 26 appels invalides (champ manquant ou mauvais type), chacun : erreur, fichier inchangé, `ping` répond ensuite ; JSON malformé / tronqué / non-objet sur stdin (9 variantes) : le serveur répond toujours.
  `security.rs` (5 tests, 1 `#[ignore]`) avec `--root` temporaire : `../evil` et `sub/../../evil` refusés ; chemin absolu hors racine refusé (`create_board` et `export`) ; dossier lien symbolique vers l'extérieur refusé ; fichier final lien symbolique vers l'extérieur : voir BUG-4 ; garde-fou vert : le fichier extérieur n'est jamais modifié. Si le lien ne peut pas être créé, le test s'arrête avec un message `SKIPPED` (stderr) et passe. Ici (Windows) les liens ont pu être créés : aucun saut.
  `scripts/mcp_e2e_claude.ps1` écrit, **non exécuté** (appel payant) ; syntaxe vérifiée avec le parseur PowerShell (0 erreur), sans l'exécuter.
- Fichiers créés / modifiés : `crates/inkboard-mcp/tests/common/mod.rs`, `tests/tools_all.rs`, `tests/security.rs`, `scripts/mcp_e2e_claude.ps1` (créés) ; `tests/session.rs` (en-tête seulement, `Client`/`temp_dir` retirés). Aucun changement dans `src/`, aucune dépendance ajoutée, `Cargo.lock` inchangé.
- Commandes et résultats : `cargo fmt --all --check` OK ; `cargo clippy --workspace --all-targets -- -D warnings` OK ; `cargo test --workspace` OK, 304 réussis, 3 ignorés (dont 1 `BUG-4` ; avant : 296 réussis, 2 ignorés). Durée 16,3 s après (cache chaud) ; la mesure « avant » n'a pas été refaite dans ce lot (25,0 s relevés à la fin du lot A, cache différent) : surcoût réel < 2 s (`tools_all` 0,23 s, `security` 1,0 s ; `session` 5,4 s inchangé).
  `cargo test -p inkboard-mcp --test security -- --include-ignored` : seul le test BUG-4 échoue.
- Bugs trouvés :
  - **BUG-4 — un fichier final lien symbolique vers l'extérieur de `--root` est suivi.**
    Entrée : `--root R`, `R/link.inkboard.json` lien vers `E/secret.inkboard.json` (hors de R), puis `describe_board {"path":"link.inkboard.json"}` et `add_shape {"path":"link.inkboard.json",…}`.
    Attendu : erreur « path outside the allowed root » dans les deux cas. Obtenu : les deux réussissent. `resolve` (`tools.rs`) ne canonicalise que le dossier parent ; la lecture suit le lien (le contenu d'un fichier hors racine est donc lisible par l'agent, s'il se laisse lire comme tableau ou import). L'écriture ne modifie pas le fichier extérieur (`serial::save` écrit un temporaire puis `rename`, qui remplace le lien lui-même par un vrai fichier dans la racine), mais copie son contenu dans la racine. `docs/MCP.md` affirme « `..` and links are neutralized » : faux pour ce cas.
    Tests : `security::symlinked_final_file_pointing_outside_the_root_is_refused` (`#[ignore]`) ; garde-fou vert `…_is_never_modified`. Piste (non appliquée) : canonicaliser le chemin entier quand il existe et revérifier `starts_with(&root)`.
- Écarts au plan et pourquoi :
  - « Erreur JSON-RPC » : le serveur renvoie les erreurs d'outil comme résultat `isError: true` (convention MCP, documentée dans `docs/MCP.md`) et réserve l'erreur JSON-RPC à l'outil inconnu (-32602), à la méthode inconnue (-32601) et au JSON invalide (-32700). Les tests acceptent l'une ou l'autre forme pour un appel invalide et vérifient -32602 pour l'outil inconnu.
  - Observation (pas un bug testé) : un champ optionnel de mauvais type est ignoré sans erreur (ex. `update_object` avec `"x": "left"` répond OK sans bouger l'objet). Ce cas a été retiré de la liste des appels invalides ; ne sont testés que les champs requis manquants / mal typés et les valeurs d'énumération inconnues (`direction`).
  - `save_board` : en mode fichier il est sans effet ; son « appel valide » vérifie seulement que le fichier reste identique. Son appel invalide est l'absence de `path` (sans application ouverte).
  - `board_changes` : après un `describe_board`, la base est déjà prise ; l'appel valide vérifie `unchanged: true`.
  - Pas de `tempfile` : `temp_dir` existant suffit (dossiers laissés dans `%TEMP%`, comme avant).
  - Étape 4 (job CI MCP Inspector, `mcp-conformance.yml`) : optionnelle, non faite.
  - Script `mcp_e2e_claude.ps1` : le tableau vide est créé par `inkboard-mcp` lui-même (`create_board`) ; option `-NoApp` pour le mode fichier ; par défaut l'application est lancée et la consigne demande `save_board`. Les options `claude` (`--mcp-config`, `--strict-mcp-config`, `--allowedTools mcp__inkboard`, `--max-turns`) ne sont pas vérifiées à l'exécution : à relire avant le premier lancement payant.
- Reste à faire / besoin d'une décision : corriger BUG-4 (lot de correction séparé, puis retirer le `#[ignore]` et corriger/confirmer la phrase de `docs/MCP.md`) ; lancer le script `mcp_e2e_claude.ps1` (appel payant) si l'utilisateur le souhaite.

### Correctif BUG-4 — 2026-10-19
- `crates/inkboard-mcp/src/tools.rs` → `resolve` : si l'entrée finale existe (`symlink_metadata`), elle est canonicalisée et doit rester sous la racine ; un lien cassé est refusé.
- `tests/security.rs` : `#[ignore]` retiré. `docs/MCP.md` et `docs/fr/MCP.md` : phrase corrigée.
- `cargo fmt`, `clippy -D warnings` OK ; `cargo test --workspace` : 305 réussis, 0 échec, 2 ignorés (aucun BUG).

### Lot C — 2026-10-19
- Fait : étape 1 validée sous Windows : `cargo run --release -p inkboard-app -- --screenshot out/win.png` quitte seul (code 0, ~1,5 s, PNG 1700×1075). Même chose vérifiée avec `--ui-demo menu` et l'ouverture d'un `.drawio` et d'un `.excalidraw` du corpus (code 0, PNG valides). `scripts/check_png.py` (stdlib : `zlib` + `struct`, PNG 8 bits non entrelacé) : code 1 si < 800×600, si < 50 couleurs distinctes sur un échantillon (1 pixel sur 3 d'une ligne sur 5), ou si le fichier est absent / illisible. `smoke.yml` écrit, **non lancé**.
- Fichiers créés / modifiés : `scripts/check_png.py`, `.github/workflows/smoke.yml` (créés). Aucun changement dans `src/`, `ci.yml`, `release.yml`.
- Commandes et résultats :
  - `check_png.py` : PNG réel → OK (58 couleurs, 0,5 s) ; PNG uniforme 1000×700 → code 1 (« 1 distinct colors ») ; PNG 400×300 → code 1 ; fichier absent → code 1.
  - Le corps du pas « Launch » du workflow, exécuté localement avec `RUNNER_OS=Windows` : 4 lancements + 4 `check_png` OK.
  - `actionlint` : indisponible (pas installé ; Docker Desktop est arrêté). Relecture + `yaml.safe_load` (PyYAML 6.0.3) : OK (5 lignes de matrice, `timeout-minutes: 15`, déclencheurs `push` sur `main` + `workflow_dispatch`).
  - `cargo fmt --all --check` OK ; `cargo clippy --workspace --all-targets -- -D warnings` OK ; `cargo test --workspace` : 305 réussis, 0 échec, 2 ignorés (11,8 s, cache chaud).
- Bugs trouvés : aucun.
- Écarts au plan et pourquoi :
  - Matrice : windows, linux (rendu par défaut), linux-wgpu, linux-glow, macos (5 lignes ; `INKBOARD_RENDERER` vide pour les 3 autres).
  - Linux : ajout de `mesa-vulkan-drivers` à la liste d'installation (lavapipe), sinon la ligne `wgpu` n'aurait pas de Vulkan logiciel et risquerait un faux échec ; `xvfb-run -a -s "-screen 0 1920x1080x24"` (l'écran par défaut 640×480×8 est trop petit pour GL et pour la fenêtre 1360×860).
  - Délai de 120 s : `/usr/bin/timeout` (Git Bash et Linux) ; macOS n'a pas `timeout` : `perl -e 'alarm 120; exec @ARGV'`.
  - Fichiers ouverts : `drawio/nogibjj__p3.drawio` et `excalidraw/JonRC__sample.excalidraw` (testés en local). Sous Windows, le build release est une application « GUI » : aucune sortie console, c'est sans importance ici.
  - PNG envoyés `if: failure()`, `retention-days: 3`, nom `smoke-<ligne>`.
- Reste à faire / besoin d'une décision : **un vrai test exige un push** (déclencheur `push` sur `main` ou `workflow_dispatch`) : à demander à l'utilisateur, non fait. Points à surveiller au premier lancement : Windows (le runner n'a probablement pas d'OpenGL 3 : repli `wgpu` attendu), `linux-wgpu` (Vulkan logiciel), macOS (session graphique du runner). Relancer `actionlint` si un poste en dispose.

### Lot D — 2026-10-19
- Fait : `crates/inkboard-core/tests/properties.rs` (API publique seulement, aucune modification de `src/`). Générateur de documents aléatoires (0 à 200 objets, 1 à 3 pages, 2 calques dont un masqué/verrouillé, formes des 6 géométries, images, tableaux, traits avec pression, connecteurs droits/coudés/courbes liés ou libres, ancres auto ou fractionnaires, groupes, objets verrouillés, quelques objets énormes), ids déterministes (`p<n>`, `o<n>`). 256 cas par propriété, graine fixe 2026 (`PROPTEST_RNG_SEED=<n>` pour explorer), pas de fichier de régressions écrit. Les 6 propriétés :
  1. `undoing_everything_restores_the_initial_document_and_redoing_the_final_one` : 0 à 25 opérations (insertion, suppression, déplacement, duplication, connexion, ordre, groupe, verrou, alignement, répartition, style, pages, fond, nom, calques, `transact`) ; annuler tout = document initial, rétablir tout = document final, même nombre de pas.
  2. `saving_then_loading_gives_an_equal_document` : `serial::save` puis `load` dans `CARGO_TARGET_TMPDIR` (dossier supprimé par un `Drop`), plus `to_json`/`from_json` compact et indenté ; flottants arrondis à 1/64 (voir BUG-5). Variante sans arrondi : `bug_5_…` (`#[ignore]`).
  3. `bound_connector_ends_stay_on_their_shape` : déplacements, rotations et redimensionnements aléatoires de formes, puis chaque extrémité liée (ancre auto) est à ≤ 0,5 px du contour (polygone ou ellipse, cadre tourné pris en compte) ; ancre fractionnaire : à ≤ 0,5 px de la position attendue (elle n'est pas sur le contour d'une ellipse, par construction).
  4. `elbow_connectors_go_around_unrelated_shapes` : grille de formes (une par case de 400 px, ±20 px), connecteurs coudés, aucun segment ne traverse l'intérieur de la boîte d'une forme non reliée (vérification indépendante du produit, découpage de Liang-Barsky). Deux variantes en `#[ignore]` (BUG-6).
  5. `index_query_equals_exhaustive_search` : `SpatialIndex::query` = recherche exhaustive (mêmes indices, même ordre) pour 1 à 11 rectangles aléatoires (dont des vides et des énormes) ; `objects_in_rect_idx` = `objects_in_rect` (contenu ou touché).
  6. Trois tests : `dragging_a_handle_then_dragging_it_back_restores_the_frame` (`resize_frame`, 8 poignées, avec/sans ratio), `rotating_then_rotating_back_restores_every_object`, `scaling_to_a_box_then_back_restores_every_object` (`scale_objects` aller-retour) ; tolérance absolue 1e-6.
  Vérifié que les tests ne sont pas vides (mesure temporaire, retirée) : 9,1 pas d'annulation en moyenne (246 cas sur 256 non vides, 66 objets en moyenne), 8 668 extrémités liées contrôlées, 1 159 requêtes sur 1 534 non vides, 715 chemins coudés sur 1 118 avec détour.
- Fichiers créés / modifiés : `crates/inkboard-core/tests/properties.rs` (créé) ; `crates/inkboard-core/Cargo.toml` (`[dev-dependencies] proptest = { version = "1.12.0", default-features = false, features = ["std"] }`, identique à inkboard-formats ; pas de nouveau paquet, seule l'entrée `inkboard-core` de `Cargo.lock` change) ; ce journal.
- Commandes et résultats : `cargo fmt --all --check` OK ; `cargo clippy --workspace --all-targets -- -D warnings` OK ; `cargo test --workspace` OK : 8 tests de propriétés réussis, 3 ignorés (BUG-5, BUG-6 ×2), aucun échec ailleurs. Durée des nouveaux tests : 5,7 s (build debug) ; `cargo test --workspace` 21,1 s en tout (11,8 s relevés au lot C, cache chaud : l'écart est surtout ces 5,7 s plus le lien du nouveau binaire de test). `cargo test -p inkboard-core --test properties -- --ignored` : les 3 tests ignorés échouent, pour les bugs ci-dessous.
- Bugs trouvés :
  - **BUG-5 — un tableau enregistré puis rechargé n'est pas égal (flottants décalés d'un ulp).**
    Entrée : un rectangle dont `frame.y = -392.15690042133724` (cas minimal trouvé par proptest, tout le reste par défaut), `serial::save` puis `serial::load`.
    Attendu : document égal. Obtenu : `y = -392.1569004213373`. Cause vérifiée à part : le fichier contient bien `-392.15690042133724` (l'écriture est exacte), `str::parse::<f64>` rend la bonne valeur, mais `serde_json::from_str` rend la valeur voisine, car la fonctionnalité `float_roundtrip` de `serde_json` est désactivée (lecture rapide mais pas toujours exacte). Gravité faible (erreur de l'ordre de 1e-16 en relatif, invisible), mais c'est un vrai écart au contrat « enregistrer puis recharger ». Piste (non appliquée) : activer `float_roundtrip` sur `serde_json`.
    Test : `bug_5_saving_then_loading_keeps_every_float` (`#[ignore]`) ; la version vivante arrondit les flottants à 1/64 (exacts en binaire) et vérifie tout le reste (structure, textes, ids, calques, images et leurs ressources, tracés avec pression).
  - **BUG-6 — le routage coudé abandonne le contournement quand le point de sortie est dans la boîte de sa propre forme.** Le connecteur traverse alors une forme non reliée.
    Entrée minimale (triangle, proptest) : cellules de la grille de 400 px, triangle 51,85 × 20 en (1000, 200), rectangle 20 × 22,3 en (1400, 200), rectangle 20 × 20 en (1800, 200), connecteur coudé du rectangle de (1800, 200) au triangle. Variante à 3 objets (vérifiée à la main) : triangle 113 × 20 en (200, 600), rectangles 20 × 20 en (600, 600) et (1000, 600), connecteur du triangle au rectangle de droite.
    Attendu : un chemin qui contourne le rectangle du milieu. Obtenu : le chemin droit `[(228,2 ; 600), (990 ; 600)]` qui le traverse.
    Entrée minimale (forme tournée, proptest) : rectangle 20 × 103,4 tourné de 0,63 rad en (200, 600), rectangles 20 × 20 en (600, 200) et (1000, 200), connecteur de la forme tournée à celui de (1000, 200) : le chemin coudé passe par (599 ; 605,9) → (599 ; 200), à travers le rectangle de (600, 200).
    Cause (lecture de `connector.rs` + vérification directe avec `route::route_around`) : `avoid_obstacles` ajoute la boîte englobante de la forme reliée aux obstacles (marge 14), puis cherche un chemin depuis `a1 = point de contour + 24 px dans l'axe du monde`. Pour un triangle (côtés E/O : point de contour à `w/4` du centre, donc `a1` dans la boîte dès que `w > 40`) ou pour toute forme tournée, `a1` est à l'intérieur de cette boîte élargie ; `route_around` renvoie `None` (« départ dans un obstacle ») et le chemin d'origine est conservé sans erreur. Vérifié : même départ sans la boîte propre → chemin de 5 points.
    Tests : `bug_6_elbow_connectors_go_around_unrelated_shapes_with_triangles` et `bug_6_elbow_connectors_go_around_unrelated_rotated_shapes` (`#[ignore]`). Le test vivant couvre rectangle, ellipse, losange, post-it et tableau non tournés (aucun échec en 256 cas).
- Écarts au plan et pourquoi :
  - Propriété 4 : le contournement n'est garanti que si un couloir libre existe et si le point de sortie est hors de la boîte de sa forme ; la scène est donc une grille de ≤ 24 formes isolées (le routeur abandonne au-delà de 60 000 nœuds, ce qui fausserait le test avec 200 formes). Au-delà de 5 000 objets, `AVOID_MAX_OBJECTS` désactive le contournement par conception (non testé). Le critère utilise la boîte englobante des formes (c'est ce que le routeur évite).
  - Propriété 1 : les objets insérés par une opération ne sont jamais des images (une ressource ajoutée par `Document::add_asset` ne fait pas partie de l'historique : annuler la laisserait ; c'est un artefact de comparaison, pas un défaut démontré du moteur) ni des connecteurs (cibles absentes). Les images du document de départ sont dupliquées normalement. `reset` est exclu (efface l'historique).
  - Propriété 3 : en plus des déplacements, rotations et redimensionnements sont appliqués ; ancres fractionnaires vérifiées par position, pas par contour.
  - Propriété 6 : « inverser » = glisser la poignée jusqu'à sa position d'origine, tourner de −a, mettre à l'échelle de `to` vers `from` ; les cas où la taille minimale a bloqué la poignée sont écartés (`prop_assume!`, non réversibles par nature), et les facteurs d'échelle sont bornés à ≥ 0,5 pour que les tableaux (minimum 20 × 16) ne soient pas écrêtés.
  - Pas de `tempfile` : `CARGO_TARGET_TMPDIR` + `Drop`.
- Reste à faire / besoin d'une décision : corriger BUG-5 (activer `float_roundtrip`, puis retirer `snap` et l'`#[ignore]`) et BUG-6 (lot de correction séparé : par exemple chercher le chemin depuis `a.pt` quand `a1` est dans la boîte propre, ou ne pas compter la boîte propre comme obstacle de départ) ; ensuite retirer les `#[ignore]` et le `triangles: bool` du générateur.

### Correctifs BUG-5/6 — 2026-10-19
- BUG-5 : `float_roundtrip` activé sur `serde_json` (`crates/inkboard-core/Cargo.toml`, fait avant ce lot). Ici : `snap` supprimée, `bug_5_…` fusionné avec la propriété 2 (`saving_then_loading_gives_an_equal_document` compare maintenant les flottants exacts, sans arrondi), `#[ignore]` retiré.
- BUG-6 : `connector.rs` → `avoid_obstacles` : avant d'appeler `route_around`, on retire de la liste d'obstacles les formes reliées (`own`) dont la boîte élargie de `CLEARANCE` contient `a1` ou `b1` (pointe du segment de sortie/entrée). 4 lignes, `route.rs` inchangé.
  Choix : c'est la cause exacte (départ « dans un obstacle » => `None` => chemin droit conservé), et le filtre ne s'active que dans ce cas : formes non tournées dont le point de sortie est hors de la boîte propre = comportement identique. Les formes non reliées restent toutes évitées ; coût nul (un filtre sur ≤ 2 rectangles, `AVOID_MAX_OBJECTS` inchangé).
  Limite assumée : dans ce cas seulement, le chemin peut longer/traverser la boîte englobante de sa propre forme reliée (pas évitée) ; non testé, aucun test existant ne le détecte.
- Tests : `#[ignore]` des 2 tests BUG-6 retirés ; le paramètre `triangles` du générateur supprimé (triangle toujours dans les formes) et la propriété 4 vivante vérifie chaque scène deux fois (formes droites, puis les mêmes tournées de leur angle) : un seul test `elbow_connectors_go_around_unrelated_shapes`.
- Résultats : `cargo fmt --all --check` OK ; `clippy -D warnings` OK ; `cargo test --workspace` OK (aucun échec, `grep 'ignore = "BUG' crates` vide) ; `properties` avec `PROPTEST_RNG_SEED=1, 2, 3` : 8 réussis chacune, aucun nouveau cas.

### Lot E — 2026-10-19
- Fait :
  - `durability.rs` (9 tests, 1 s) : fichier tronqué à chaque octet (en mémoire, 2 formats) et à 40 positions via le disque → `Err` lisible (une ligne, non vide), jamais de panic ; fichiers vides / non JSON ; version trop récente (`TooNew`, message « newer version »), format étranger, versions et champs mal typés ou absents (erreur JSON nommant le champ) ; 9 contenus mal typés (type d'objet inconnu, `pages` objet, etc.) ; 400 corruptions aléatoires (graine fixe) sans panic ; champs inconnus (envelope, document, pages, objets) ; enregistrement impossible : le nom du fichier temporaire est occupé par un dossier (échoue partout) → `Err`, cible octet pour octet intacte ; dossier en lecture seule (voir écarts) ; dossier absent ; fichier `*.tmp-save` laissé (à moitié écrit, vide, complet, zéros) → la cible reste lisible et inchangée, l'enregistrement suivant réussit et consomme le temporaire.
  - `kill_during_save.rs` : `ink` tué 10 fois (graine fixe, 4 essais sur 10 sans ancienne version) sur un tableau de 10 000 objets, moitié à un instant aléatoire de l'exécution, moitié juste après le premier changement vu sur la cible ou son temporaire (attente active, jitter 0 à 1,5 ms). Cible toujours absente / ancienne version complète / nouvelle version complète. Test vivant sur `ink bench` (passe par `serial::save`, comme l'application) ; test sur `ink convert` : voir BUG-7. Garde-fou contre un test creux : au moins 3 tués en cours d'exécution (mesuré : 8 à 10).
  - Test `egui_kittest` `opening_a_corrupted_file_shows_an_error_and_keeps_the_current_board` (fin de `tests.rs`) : 6 fichiers corrompus (tronqué, binaire, trop récent, format étranger, `.drawio` cassé, non UTF-8) + fichier absent → `toast` non vide, document identique, révision de l'historique inchangée, `path` inchangé. L'API permet de l'observer (`app.toast`, `app.ed.doc`) : pas de saut.
  - `.github/workflows/bench.yml` : `workflow_dispatch` + cron hebdomadaire (lundi 04:17 UTC), ubuntu-latest, `ink bench 20000` ×5, médiane (`sort | sed -n 3p`) contre `THRESHOLD_MS=552`, échec au-delà. Non lancé. YAML relu et chargé avec PyYAML.
- Fichiers créés / modifiés : `crates/inkboard-core/tests/durability.rs`, `crates/inkboard-cli/tests/kill_during_save.rs`, `.github/workflows/bench.yml` (créés) ; `crates/inkboard-app/src/tests.rs` (un test ajouté à la fin) ; ce journal. Aucun changement dans `src/` du produit hors ce `tests.rs` (module de test), aucune dépendance ajoutée, `Cargo.lock` inchangé.
- Commandes et résultats :
  - `cargo fmt --all --check` OK ; `cargo clippy --workspace --all-targets -- -D warnings` OK ; `cargo test --workspace` OK, 0 échec, ignorés : le test BUG-7 (+ les 2 déjà présents hors lot). Durée totale ~42 s, contre ~21 s au lot D (cache chaud, machine non isolée : les 16 s du lot D de `inkboard-formats`/`properties` et les 5 s de `session` MCP dominent). Nouveaux tests : `durability` 1,0 s, `kill_during_save` 3 à 5 s (un seul test vivant), test d'application 0,1 s. Total ajouté < 6 s.
  - Mesure de référence du garde-fou, build release local Windows 11 : `ink bench 20000` = 92 ms de médiane (7 mesures : 89 à 98 ms ; 481 ms à froid la 1re fois). Seuil 3 × 92 × 2 = 552 ms. **Non mesuré sur un runner GitHub** : le premier lancement dira si 552 ms est trop lâche ou trop serré (écrit dans le commentaire du workflow).
  - Le `Drop` du dossier temporaire de `durability.rs` remet le dossier en écriture avant de le supprimer.
- Bugs trouvés :
  - **BUG-7 — `ink convert` vers un fichier natif écrit la cible sur place (non atomique).**
    Entrée : `ink convert gros.inkboard.json cible.inkboard.json` tué pendant l'écriture. Attendu (comme `serial::save`, dont le commentaire promet « a crash never leaves a truncated file ») : cible absente, ancienne ou nouvelle version complète. Obtenu (lecture du code, `crates/inkboard-cli/src/main.rs`, fin de `Cmd::Convert`) : `std::fs::write(&output, text)` ouvre en troncature puis écrit : l'ancienne version est perdue dès l'ouverture, et un arrêt en cours d'écriture laisse un fichier tronqué. Même chose pour les sorties `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`. L'application n'est pas touchée (elle passe par `serial::save`, vérifié par le test vivant).
    **Non reproduit** : sous Windows, en 4 séries de 10 essais (plus des essais à 10 000 et 40 000 objets, sortie de 16 à 64 Mo, instrumentés), l'écriture d'un fichier entier n'a jamais été vue à moitié faite : la taille vue sur la cible passe directement de l'ancienne valeur à la valeur finale (la fin de fichier est étendue d'un coup) et le tuage tombe toujours avant ou après l'écriture. Le test ne peut donc pas distinguer ici un `fs::write` d'un enregistrement atomique, et, à l'inverse, pourrait échouer de façon intermittente là où un tuage peut interrompre une grosse écriture (non vérifié sous Linux/macOS, hypothèse). Pour ne pas livrer un test potentiellement instable, il est `#[ignore = "BUG-7 …"]` ; lancement : `cargo test -p inkboard-cli --test kill_during_save -- --ignored`, à tenter sur Linux/macOS. Piste (non appliquée) : écrire dans `<cible>.tmp-save` puis `rename`, comme `serial::save`.
- Écarts au plan et pourquoi :
  - Champs inconnus : le plan attend `Err` ; le produit les **ignore** (pas de `deny_unknown_fields`), ce qui est le bon comportement pour un fichier d'une version mineure plus récente. Le test vérifie donc `Ok` et un document identique (rien de perdu ni de mal interprété). À confirmer comme décision de conception ; les types inconnus ou mal typés, eux, donnent `Err`.
  - Dossier en lecture seule : sous Windows (et en administrateur sous Unix) le dossier reste inscriptible : le test imprime `SKIPPED read-only folder …` (constaté ici) et passe. La partie fiable partout (nom du temporaire occupé par un dossier) est toujours exécutée et vérifie la même chose (`Err` + cible intacte).
  - Limite générale du test de tuage : il valide `serial::save` (aucune cible à moitié écrite, y compris autour du `rename`), mais ne simule pas une coupure de courant (cache disque non vidé : `save` ne fait pas de `fsync`, voir ci-dessous).
  - Chaînes sans accent dans les sources de test : un test existant (`no_french_is_left_in_the_source_outside_the_translation_table`) parcourt tous les `.rs` ; les accents sont écrits en `\u{…}`.
- Reste à faire / besoin d'une décision :
  - Corriger BUG-7 (écrire de façon atomique dans `ink convert`) puis retirer l'`#[ignore]`, ou accepter le comportement (outil en ligne de commande).
  - Décider du comportement attendu pour les champs inconnus (ignorés aujourd'hui).
  - Observation (pas un bug testé) : `serial::save` n'appelle pas `sync_all` avant le `rename` ; un arrêt brutal de la machine (pas du processus) peut, sur certains systèmes de fichiers, laisser un fichier vide après `rename`. Non testable sans coupure réelle.
  - Lancer `bench.yml` (push / `workflow_dispatch`, à demander) et recaler `THRESHOLD_MS` sur une mesure du runner.

### Correctif BUG-7 — 2026-10-19
- `crates/inkboard-core/src/serial.rs` : nouvelle `write_atomic(path, bytes)` (fichier `.tmp-save`, `sync_all`, `rename`) ; `save` l'utilise (ajoute donc le `sync_all` signalé au lot E).
- `crates/inkboard-cli/src/main.rs` : les trois écritures de `ink convert` (PNG, PDF, texte) passent par `write_atomic`.
- `tests/kill_during_save.rs` : `#[ignore]` retiré. Le bug restait non reproduit sous Windows (lecture du code seulement).
