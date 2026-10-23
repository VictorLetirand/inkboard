# Plan — MCP v2 : ce qui manque à un agent IA pour travailler sur une vraie planche

*Rédigé le 2026-10-20, après un essai réel du serveur MCP (planche jetable : Mermaid + tableau + post-it + export PNG relu).*
*Base : `main` à `c58b8d5` (rc1 publiée). Exécutant : agents Sonnet 5.5 (palier T1), un par lot, dans des worktrees séparés. Relecture et intégration : l'agent principal.*

---

## 0. Pourquoi, et ce qu'on ne fait pas

Le serveur MCP actuel (16 outils, mode fichier + mode direct, `board_changes`, confinement `--root`) **fonctionne** : création, relecture, export, correction par l'image.
Cinq manques ressortent à l'usage ; ce plan les comble, dans cet ordre de valeur :

| Lot | Manque | Résultat attendu pour l'agent |
|---|---|---|
| **1** | Pas d'icônes (28 800 dans l'application, aucune accessible) | Chercher et poser une icône AWS / Azure / Cisco / brand… sur la planche |
| **2** | Pour vérifier son travail il faut exporter un fichier puis le relire | Un outil `render` qui **renvoie l'image directement** (avec, en option, les identifiants dessinés sur les objets) |
| **3** | Lecture sans styles ; pas de déplacement groupé, d'alignement, de regroupement, d'ordre d'empilement, de gestion des pages, de recherche ; flèches non modifiables | Lire les styles, retoucher une planche existante, modifier flèches et pages |
| **4** | Pas de gras / couleur / surlignage par mot | Texte riche à la création et à la modification, relu fidèlement |
| **5** | Un appel par objet : lent, verbeux, pas d'annulation en bloc fiable | `batch` : plusieurs opérations en un appel, références entre elles, tout-ou-rien |

**Hors périmètre (à ne pas faire dans ces lots, notés en §9)** : cylindre Mermaid, tracé du retour « Page de connexion → Client », mesure de texte réelle en mode direct, lecture des traits de stylet, collaboration distante.

**Contraintes d'identité du projet (non négociables)** : hors ligne (aucun téléchargement), transport stdio, confinement des chemins à `--root`, une erreur d'outil ne doit jamais corrompre ni modifier le fichier, messages d'erreur en anglais exploitables par l'agent, compatibilité ascendante des 16 outils existants (mêmes noms, mêmes paramètres, mêmes sorties — on peut **ajouter** des champs, jamais en retirer ni changer leur sens).

---

## 1. État du code (vérifié le 2026-10-20)

- `crates/inkboard-mcp/src/tools.rs` (763 lignes) : `list()` (schémas), `apply(ed, name, args)` (exécute sur un `Editor` en mémoire), `call(srv, name, args)` (résout les chemins sous `--root`, tente le mode direct via `live::forward`, sinon charge/applique/enregistre le fichier), `apply_tracked` (suivi `board_changes`), `summary(o)` (JSON d'un objet), helpers `s/os/of/ocolor/find/origin/obj/pos_props/path_prop/page_prop`.
- `apply` est **aussi** exécuté par l'application en mode direct (`crates/inkboard-app/src/live.rs:157`, dans `ed.transact` → **un appel d'outil = un pas d'annulation**). Tout ce qu'on ajoute à `apply` marche donc dans les deux modes sans autre câblage.
- Le résultat d'un outil est une valeur JSON ; `lib.rs` l'enveloppe en `{"content":[{"type":"text","text":…}]}`. Il passe tel quel dans le protocole direct (une ligne JSON par réponse).
- `Editor` (`crates/inkboard-core/src/editor.rs`) sait déjà : `translate`, `align(AlignMode)`, `distribute(Axis)`, `reorder(Order)`, `group`, `ungroup`, `set_locked`, `rename_page`, `remove_page`, `update_many`, `remove(&[Id])`, `duplicate`. **Le moteur est prêt ; c'est l'exposition MCP qui manque** (lot 3).
- Style d'objet (`crates/inkboard-core/src/style.rs`) : `stroke, fill, width, dash, opacity, roundness`. Texte : `Label { text: RichText, family, size, align }` ; `RichText = Vec<Span>`, `SpanStyle { bold, italic, underline, strike, color, highlight }` (`crates/inkboard-core/src/text.rs`) ; `RichText::apply_format(from, to, Format)` existe déjà.
- Icônes : `crates/inkboard-icons` (`Library::open`, `search`, `svg`, `name`, `groups`, `sets[i].mono`, `svg_dimensions`). Le pack `crates/inkboard-app/assets/icons.bin` (≈ 8 Mo) est embarqué par `crates/inkboard-app/src/library.rs` (`pub static PACK`). Une icône posée = `ImageObj { frame, asset, latex: None, mono }` ; `Document::add_asset(mime, bytes)` dédoublonne ; les icônes monochromes sont stockées en blanc et teintes par `style.stroke` (`ImageObj.mono`, `export_bytes`).
- Export : `inkboard_core::svg::export(doc, page, selection?, measurer, &SvgOptions)` puis `inkboard_formats::raster::svg_to_png(svg, scale)`.
- Mesure de texte du MCP : `ApproxMeasurer` partout (aussi en mode direct) — c'est volontaire dans ce plan de ne pas y toucher.
- Tests existants à connaître : `crates/inkboard-mcp/tests/{common/mod.rs,tools_all.rs,session.rs,security.rs}` (client MCP écrit pour les tests qui pilote le vrai binaire avec `--root … --no-live`) ; tests du mode direct dans `crates/inkboard-app/src/tests.rs` (helper `Live`, ~ligne 1190). `tools_all.rs` contient `TOOLS` (16 noms) et `tools_list_returns_the_16_documented_tools` qui exige **exactement** ces 16 outils.

---

## 2. Organisation du travail

### 2.1 Phase 0 — la « couture » (faite par l'agent principal, avant tout lot)

Cinq lots qui modifient tous `tools.rs` produiraient des conflits à la fusion. On crée donc **une fois** une couture minimale (≈ 30 lignes) :

1. Dans `tools.rs` : `mod batch; mod edit; mod icons; mod render; mod richtext;` → fichiers `crates/inkboard-mcp/src/tools/{batch,edit,icons,render,richtext}.rs`, **vides sauf le contrat** :
   ```rust
   pub(super) const NAMES: &[&str] = &[];        // outils exposés par ce module
   pub(super) const READ_ONLY: &[&str] = &[];    // ceux qui ne modifient pas la planche
   pub(super) fn specs() -> Vec<Value> { Vec::new() }                                   // schémas pour tools/list
   pub(super) fn apply(_ed: &mut Editor, _name: &str, _a: &Value) -> Option<Res<Value>> { None }
   ```
   (`richtext.rs` n'expose pas d'outil : c'est une bibliothèque d'aide ; pas de `NAMES`.)
2. `list()` concatène `specs()` des modules ; `call()` et `apply()` acceptent les noms des modules (au lieu de `EDITOR_TOOLS` seul) ; `READ_ONLY` est étendu par les `READ_ONLY` des modules ; `apply()` appelle les modules avant le `match` historique. Les modules accèdent aux helpers privés du parent via `super::`.
3. `tools_all.rs` : `tools_list_returns_the_16_documented_tools` devient « contient **au moins** les 16 outils documentés, sans doublon, chacun avec description non vide et `inputSchema.type == "object"` » (l'égalité exacte est rétablie à l'intégration, §6).
4. Aucun comportement ne change : tous les tests existants passent. Commit sur la branche `feat/mcp-v2` créée depuis `c58b8d5`.

C'est le seul « refactor » du plan ; il est justifié par le parallélisme et reste mécanique.

### 2.2 Vagues, branches, isolation

Chaque agent travaille dans **son worktree**, sur sa branche créée depuis `feat/mcp-v2` (après la phase 0) :

| Vague | Lot | Branche | Peut tourner avec |
|---|---|---|---|
| **V1** | 1 Icônes | `feat/mcp-icons` | 2 et 4 |
| **V1** | 2 Rendu image | `feat/mcp-render` | 1 et 4 |
| **V1** | 4 Texte riche | `feat/mcp-richtext` | 1 et 2 |
| **V2** (après fusion de V1 dans `feat/mcp-v2`) | 3 Lecture + édition | `feat/mcp-edit` | 5 |
| **V2** | 5 Lot `batch` | `feat/mcp-batch` | 3 |

Pourquoi cet ordre : le lot 4 modifie `add_shape / add_table / update_object / add_connector / summary` (traitement du texte) ; le lot 3 modifie aussi `summary` et `update_object` (styles) → le 3 part du résultat du 4. Le lot 5 est générique (il appelle `apply` par nom) mais ne se teste vraiment qu'avec les outils des autres lots présents.

### 2.3 Fichiers autorisés par lot (tout autre fichier = hors lot, ne pas l'ouvrir)

| Lot | Peut modifier / créer | Ne touche **pas** |
|---|---|---|
| 1 | `crates/inkboard-mcp/src/tools/icons.rs`, `crates/inkboard-mcp/Cargo.toml`, `crates/inkboard-icons/{Cargo.toml,src/lib.rs}` (+ `src/embedded.rs` si besoin), `crates/inkboard-app/src/library.rs` (n'utiliser que le pack partagé), `crates/inkboard-mcp/tests/icons.rs` | `tools.rs` (hors lecture), `lib.rs`, docs |
| 2 | `crates/inkboard-mcp/src/tools/render.rs`, `crates/inkboard-mcp/src/lib.rs` (conversion de l'image en contenu MCP), `crates/inkboard-mcp/tests/render.rs`, `crates/inkboard-core/src/svg.rs` **seulement** si une option de région/identifiants est impossible autrement | `tools.rs`, docs |
| 4 | `crates/inkboard-mcp/src/tools/richtext.rs`, `crates/inkboard-mcp/src/tools.rs` (uniquement les fonctions de traitement du texte et `summary`), `crates/inkboard-mcp/tests/richtext.rs`, `crates/inkboard-core/src/text.rs` **seulement** pour ajouter des helpers purs (ex. `to_markdown`) | `lib.rs`, docs |
| 3 | `crates/inkboard-mcp/src/tools/edit.rs`, `crates/inkboard-mcp/src/tools.rs` (uniquement `summary`, `describe`, `update_object`), `crates/inkboard-mcp/tests/edit.rs` | `lib.rs`, `editor.rs`, docs |
| 5 | `crates/inkboard-mcp/src/tools/batch.rs`, `crates/inkboard-core/src/editor.rs` (**uniquement** `try_transact`, §3.5), `crates/inkboard-mcp/tests/batch.rs`, `crates/inkboard-mcp/tests/security.rs` (ajouter des cas) | `tools.rs`, `lib.rs`, docs |

**Docs** (`docs/MCP.md`, `docs/fr/MCP.md`, `README*`) : aucun lot ne les édite (conflits garantis sur le tableau des outils). Chaque lot dépose une section prête à coller dans **`context/mcp-v2/lot-N-doc.md`** (EN, même style que `docs/MCP.md`) ; l'agent principal les intègre à la fin (§6).
**Tests partagés** : aucun lot ne modifie `tools_all.rs` (sauf phase 0 et intégration) ; chaque lot a **son propre fichier de tests** dans `crates/inkboard-mcp/tests/`.

### 2.4 Règles communes (à recopier dans chaque brief d'agent)

- Code et commentaires en **anglais**, comme le reste du dépôt ; messages d'erreur d'outils en anglais ; réponses de l'agent à l'utilisateur en français, **concises**.
- Diff minimal, **aucun refactor non demandé**, pas de reformatage de lignes non concernées ; style du dépôt (`rustfmt.toml`, commentaires rares et utiles).
- Pas de Bash heredoc pour écrire du code (guillemets/antislashs mutilés) : outils Write / Edit.
- Windows : `cargo test -p x -- nom_du_test` (le filtre va **après** `--`). **Ne pas** lancer `cargo build --release --workspace` (le binaire `target\release\inkboard-mcp.exe` peut être verrouillé par un serveur MCP en cours d'exécution → « Accès refusé ») ; rester en debug.
- Pour itérer : `cargo test -p inkboard-mcp -p inkboard-core -p inkboard-formats -p inkboard-icons` (léger, sans egui). La suite complète (`cargo test --workspace`) **une seule fois** à la fin.
- Portes de sortie, toutes obligatoires avant de rendre la main :
  1. `cargo fmt --all`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo +1.99.1 clippy --workspace --all-targets -- -D warnings`
  4. `cargo test --workspace` (tout vert ; les tests `#[ignore]` restent ignorés)
- Chaque outil neuf a : schéma complet (`description` utile **pour un LLM** : quand l'utiliser, valeurs par défaut, limites), validation d'entrée avec message d'erreur actionnable, test « chemin heureux » **et** test d'erreur qui vérifie que **le fichier n'a pas changé**, test que le serveur répond encore (`ping`) après l'erreur, cas limites (liste vide, id inconnu, valeur hors plage, très grande entrée).
- Chaque outil qui écrit passe par `Editor` (jamais `doc` directement) pour rester annulable en mode direct (**un appel = un pas d'annulation**).
- Tout chemin (`path`, `output`, `source`) passe par le confinement existant ; **ne jamais ouvrir un fichier sans `resolve`**.
- Commit sur la branche du lot, identité du dépôt (déjà configurée : `Victor Letirand <48204115+VictorLetirand@users.noreply.github.com>`, **jamais** une autre adresse), message en anglais impératif, terminé par :
  `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. **Pas de push.**
- Rapport final (≤ 25 lignes) : fichiers touchés, décisions de conception prises (et pourquoi), schémas des outils, noms des tests ajoutés, ce qui n'a **pas** été vérifié. Ne rien affirmer qui n'a pas été exécuté.

---

## 3. Spécification détaillée des lots

Notation : `[req]` = obligatoire, sinon optionnel ; toutes les coordonnées sont en unités du modèle (comme `x, y, w, h` existants) ; tous les outils acceptent `path` et `page` comme les outils actuels (mêmes helpers `path_prop()` / `page_prop()`).

### 3.1 Lot 1 — Icônes (`tools/icons.rs`)

**Outils**

1. `search_icons` (lecture seule)
   - Entrée : `query` [req] (texte libre, insensible à la casse/accents comme la palette de l'application), `group` (ex. « AWS », « Brands », « General icons »), `set` (nom exact d'un jeu), `limit` (défaut 20, max 100).
   - Sortie : `{"count": n, "truncated": bool, "icons": [{"id": "…", "name": "…", "set": "…", "group": "…", "mono": bool}]}`.
   - `id` : chaîne **stable et unique** qui permet de retrouver l'icône exacte (suggestion : `"<jeu>/<nom>"`, avec suffixe `#k` seulement si le couple n'est pas unique dans le jeu — **à vérifier dans `Library`**, documenter la décision).
2. `list_icon_groups` (lecture seule)
   - Sortie compacte : `{"total": 28824, "groups": [{"group": "AWS", "sets": [{"set": "…", "icons": n}]}]}` (pour que l'agent sache quoi chercher sans tout lister).
3. `add_icon`
   - Entrée : `icon` [req] (l'`id` de `search_icons`), `x`, `y` (défaut : sous le contenu, comme les autres), `size` (côté le plus long, défaut 64, entre 8 et 2000) **ou** `w`/`h` (le ratio de l'icône est conservé si un seul est donné), `stroke` (couleur de teinte pour les icônes monochromes ; ignorée avec message dans `warnings` pour les icônes en couleurs), `label` (texte placé sous l'icône : crée une zone de texte libre centrée sous l'icône, **groupée** avec elle via `Editor::group`).
   - Sortie : `{"id": "…", "w": n, "h": n, "icon": "<id>", "label_id": "…"?}` ; `id` est l'objet image (connectable avec `add_connector` : `is_attachable`).
   - Erreur si l'icône est inconnue : message avec **jusqu'à 5 suggestions** proches (« icon not found: "ec2" ; did you mean: … »).

**Conception imposée**

- Pack partagé sans doublon : `inkboard-icons` gagne une feature **`embedded`** (désactivée par défaut) qui expose `pub static PACK: &[u8] = include_bytes!("../../inkboard-app/assets/icons.bin")` et `pub fn shared() -> Option<&'static Library>` (même `OnceLock` que `library.rs`). `inkboard-mcp` active la feature ; `inkboard-app/src/library.rs` **utilise `inkboard_icons::PACK` / `shared()`** à la place de son propre `include_bytes!` — sinon le binaire de l'application embarquerait **le pack deux fois (+8 Mo)**. Vérifier la taille de `target/release/inkboard.exe` avant/après si possible (sinon l'écrire dans « non vérifié »).
- Poser l'image exactement comme la palette de l'application : copier la logique de `library.rs` (SVG → `Document::add_asset("image/svg+xml", …)`, `ImageObj { mono, … }`, style `width: 0.0, fill: None`). Ne pas dupliquer de code de rendu.
- Le serveur MCP autonome grossit d'environ 8 Mo : acceptable, à écrire dans la doc.

**Tests (`tests/icons.rs`, vrai binaire, `--no-live`)**
`search_icons` trouve « lambda » dans « AWS » et « github » dans « Brands » ; `limit` respecté ; `group`/`set` filtrent ; requête vide → erreur claire ; `list_icon_groups` somme = nombre total ; `add_icon` : l'objet est une image avec le bon actif SVG, `mono` correct selon le jeu, ratio conservé, `size`/`w`/`h`, `stroke` teinte (export SVG contient la couleur et plus `#ffffff`), `label` crée un groupe de 2 objets ; icône inconnue → erreur avec suggestions et **fichier inchangé** ; poser deux fois la même icône → un seul actif (dédoublonnage) ; connecteur entre deux icônes possible ; `export` PNG d'une planche d'icônes non vide. Test du mode direct dans `inkboard-app/src/tests.rs` **non requis** (la logique est dans `apply`) ; en revanche `cargo test -p inkboard-app` doit rester vert (changement de `library.rs`).

### 3.2 Lot 2 — Rendu d'image direct (`tools/render.rs`, `lib.rs`)

**Outil `render`** (lecture seule ; ne modifie ni le fichier ni la planche affichée ni l'historique)

- Entrée : `page`, `region` `{x, y, w, h}` (rectangle du modèle à montrer ; défaut : tout le contenu), `ids` (liste d'objets : on cadre dessus avec `padding`), `padding` (défaut 24), `max_px` (côté le plus long de l'image, défaut 1400, max 3000), `show_ids` (défaut false), `output` (optionnel : écrit **aussi** le PNG dans ce fichier, confiné à `--root`, pour les clients qui n'affichent pas les images).
- `region` et `ids` ensemble → erreur « give either region or ids ». `ids` inconnu → erreur listant les inconnus.
- Sortie : **bloc image** MCP (`{"type":"image","data":<base64>,"mimeType":"image/png"}`) suivi d'un bloc texte JSON `{"width_px", "height_px", "region": {x,y,w,h}, "scale", "ids_shown": n, "output"?: "…"}`.
- `show_ids: true` : sur une **copie** du document, ajouter par-dessus chaque objet (sauf traits de stylet) une petite pastille portant son identifiant (police 11–12, fond blanc, bordure fine ; position : coin haut-gauche de l'objet, milieu pour une flèche) pour que l'agent relie « ce que je vois » à « l'id à passer à `update_object` ». La planche réelle n'est **pas** touchée.
- Garde-fous : planche vide → erreur « the page is empty: nothing to render » ; image encodée > 4 Mio → réduire `max_px` automatiquement (et le dire dans `scale`/texte) plutôt qu'échouer.

**Convention de transport (aucun changement de protocole direct)**
Le résultat de `apply` est un JSON ; pour une image il contient la clé réservée `"_image": {"mime": "image/png", "data_base64": "…"}`. `lib.rs` (`tools/call`) la détecte : contenu = `[image, texte(JSON sans `_image`)]`. Les autres outils sont inchangés. Le protocole entre `inkboard-mcp` et l'application (une ligne JSON) transporte la clé telle quelle (la ligne peut dépasser 4 Mio : vérifier que `read_line` côté `live.rs` n'a pas de limite implicite, sinon l'écrire en « non vérifié »).

**Tests (`tests/render.rs`)**
Signature PNG et dimensions ≤ `max_px` ; `region` rogne (dimensions et pixels différents) ; `ids` cadre et `padding` agrandit ; `show_ids` change l'image **et** la planche (octets du fichier `.inkboard`) reste identique ; `region`+`ids` → erreur ; page vide → erreur ; `output` écrit un PNG valide dans `--root` et refuse hors root (`security.rs`-style : ajouter le cas dans **ce** fichier de tests, pas dans `security.rs`) ; le bloc image est bien le 1er `content` et le texte est du JSON valide ; ajouter l'appel `render` à une session `tools/call` complète avec le vrai binaire (`tests/common` : le client sait lire `content`).
Mode direct : un test dans `crates/inkboard-app/src/tests.rs` **non autorisé** dans ce lot → à noter dans « non vérifié » ; l'intégration (§6) l'ajoute.

### 3.3 Lot 4 — Texte riche (`tools/richtext.rs`, `tools.rs`)

**Principe** : ne pas casser l'existant → `text` reste du texte brut par défaut. On ajoute un paramètre booléen **`markdown`** (défaut false) aux outils qui prennent du texte, et un nouvel outil pour retoucher du texte déjà posé.

**Syntaxe `markdown: true`** (sous-ensemble volontairement petit, que les LLM écrivent naturellement)
`**gras**`, `*italique*` (ou `_italique_`), `__souligné__`, `~~barré~~`, couleur/surlignage : `{color=#e03131}texte{/}`, `{bg=#ffec99}texte{/}`, `{color=ink bg=#ffec99}texte{/}` ; imbrication autorisée ; `\` échappe `* _ ~ { } \` ; une syntaxe invalide ou non fermée → **texte littéral** conservé (jamais d'erreur ni de perte), avec un champ `warnings` dans la sortie quand un balisage a été ignoré. Sauts de ligne `\n` inchangés.

**Où** : `add_shape`, `update_object` (texte d'une forme ou d'une flèche), `add_table` (chaque cellule), `add_connector` (`label`), `update_object` d'une cellule de tableau n'existe pas aujourd'hui → **hors lot** (lot 3).

**Nouvel outil `format_text`** (modifie du texte déjà posé, comme la sélection dans l'application)
- Entrée : `id` [req], `match` (sous-chaîne à mettre en forme ; toutes les occurrences si `all: true`, sinon la première) **ou** `range` `[début, fin]` (indices de caractères du texte brut, début inclus, fin exclue), `bold`, `italic`, `underline`, `strike` (booléens ; `false` retire), `color`, `highlight` (couleur `#rrggbb` / `"ink"` ; `null` retire), `clear: true` (supprime toute mise en forme de la zone), `cell` `[ligne, colonne]` pour un tableau (indices à partir de 0).
- S'appuie sur `RichText::apply_format` / `restyle` du cœur ; une seule modification d'historique ; `match` introuvable → erreur « text not found » avec un extrait du texte réel.
- Sortie : `{"id": "…", "changed": n_caractères}`.

**Lecture fidèle**
- `summary()` ajoute `"markdown"` **uniquement** si le texte contient une mise en forme (sinon rien ne change : les sorties actuelles restent identiques octet pour octet). Pour un tableau : `"cells_markdown"` seulement si au moins une cellule est formatée. Fonction pure `to_markdown(&RichText) -> String` (et son inverse `from_markdown(&str) -> (RichText, Vec<String>)`) dans `richtext.rs`, testée en **aller-retour** : `from(to(x)) == x` pour des RichText aléatoires (test de propriétés avec `proptest` si déjà en dev-dependency du crate ; sinon boucle déterministe sur ≥ 500 cas générés).
- `fit` : après changement de texte la forme est ajustée comme aujourd'hui (`b.fit(&ApproxMeasurer)`), le gras compte dans la largeur (vérifier que `layout_rich` gère déjà `bold`).

**Tests (`tests/richtext.rs`)**
Markdown → spans corrects (gras, italique, souligné, barré, couleur, surlignage, imbriqué) ; échappements ; balisage non fermé → littéral + `warnings` ; `markdown: false`/absent → texte inchangé (régression : `**x**` reste `**x**`) ; tableau avec cellules formatées ; étiquette de flèche formatée ; `format_text` par `match`, `all`, `range`, `clear`, `cell`, retrait (`false`/`null`) ; `describe_board` montre `markdown` pour du texte formaté et **n'ajoute rien** pour du texte brut ; aller-retour ; erreurs avec fichier inchangé ; une planche formatée s'exporte en SVG/PNG sans erreur et le SVG contient la couleur demandée ; annulation en mode direct non testée ici (note).

### 3.4 Lot 3 — Lecture des styles + édition (`tools/edit.rs`, `tools.rs`)

**3.4.a Lecture** — `describe_board` gagne `detail` : `"brief"` (défaut, sortie **identique** à aujourd'hui) ou `"full"`. En `full`, chaque objet a en plus : `style` `{stroke, fill, width, dash, opacity, roundness}` (couleurs `#rrggbb`/`"ink"`/`null`), `font` `{family, size, align}` (formes et étiquettes), `z` (index d'empilement dans la page), `group` (id) et `locked` quand présents, `rotation` si le modèle en a une, et pour une flèche : `route`, `arrow` (`end|start|both|none`), `stroke`. `selection` inchangé. Un nouvel outil léger **`find_objects`** (lecture seule) : `text` (sous-chaîne, insensible à la casse), `regex` (alternative, taille limitée, durée bornée), `type` (`rect|ellipse|…|connector|table|image|formula|pen`), `in_region` `{x,y,w,h}`, `limit` (défaut 50, max 500) → `{"count","truncated","objects":[summary…]}`.

**3.4.b `update_object` étendu** (les paramètres actuels restent) :
- `stroke_width`, `dash` (`solid|dashed|dotted` — noms exacts du modèle), `opacity` (0–1), `roundness` (0–1), `font_size`, `font_family` (noms du modèle), `align` (`left|center|right`), `locked`, `rotation` (si le modèle l'expose ; sinon **ne pas l'ajouter**, le dire).
- Flèche : `route`, `arrow`, `label` (existant), `from`, `to` (rebrancher sur d'autres objets attachables, mêmes validations que `add_connector`).
- Cellule de tableau : `cell` `[ligne, colonne]` + `text` (le lot 4 a déjà fourni `markdown`).
- Valeur hors plage ou inconnue → erreur nommant le paramètre et les valeurs admises ; **aucune modification partielle** : valider tout avant d'appliquer.
- La sortie garde `avertissements` (clé existante, **ne pas renommer**) ; on peut ajouter `warnings` avec le même contenu, rien d'autre.

**3.4.c Nouveaux outils d'édition** (noms volontairement peu nombreux pour limiter le coût en contexte des schémas)
1. `move_objects` : `ids` [req] (1 à 2000), `dx`, `dy` (déplacement relatif) **ou** `x`,`y` (coin haut-gauche de la boîte englobante du lot) → `Editor::translate`. Les flèches liées suivent.
2. `arrange_objects` : `ids` [req], `action` [req] parmi `align_left|align_center_x|align_right|align_top|align_center_y|align_bottom|distribute_horizontal|distribute_vertical|bring_to_front|bring_forward|send_backward|send_to_back|group|ungroup|lock|unlock` → `Editor::align/distribute/reorder/group/ungroup/set_locked`. Erreur claire si `distribute` avec < 3 objets (documenter ce que fait le moteur).
3. `manage_pages` : `action` [req] parmi `rename|delete|list`, `page` (numéro ou nom), `name` (pour `rename`) → `Editor::rename_page/remove_page`. Refuser de supprimer la dernière page. (`add_page` existe déjà.)
4. `delete_object` accepte en plus `ids` (liste) — compatibilité : `id` seul marche toujours ; ni `id` ni `ids` → erreur. Sortie : `{"deleted": id}` pour `id`, `{"deleted": [ids]}` pour `ids`.
5. `duplicate_objects` : `ids` [req], `dx`, `dy` (défaut 40, 40), `count` (défaut 1, max 50) → `Editor::duplicate` ; renvoie les nouveaux identifiants (les flèches entre objets dupliqués sont conservées si le moteur le fait déjà — le vérifier, ne pas le réinventer).

**Tests (`tests/edit.rs`)**
`describe_board` en `brief` identique à l'ancien (comparaison sur une planche de référence) ; `full` contient les champs attendus pour chaque type d'objet ; `find_objects` par texte/regex/type/région, limites, regex invalide ; chaque paramètre de `update_object` (valeur valide, hors plage, inconnue, **aucune modification partielle** : fichier identique après erreur) ; rebranchement de flèche ; cellule de tableau ; `move_objects` (les flèches liées suivent) ; chaque `action` de `arrange_objects` avec un résultat numérique vérifié (positions après alignement/répartition, ordre d'empilement via `z`, groupe partagé, verrou : un objet verrouillé refuse `update_object` ? — **vérifier le comportement actuel du moteur et le documenter**, ne pas l'inventer) ; `manage_pages` (renommer, supprimer, dernière page refusée, `list`) ; `delete_object` avec `ids` ; `duplicate_objects`. Test d'annulation en mode direct : **non autorisé** dans ce lot (noté), ajouté à l'intégration.

### 3.5 Lot 5 — `batch` (`tools/batch.rs`, `editor.rs`)

**Outil `batch`**
- Entrée : `operations` [req] (liste de 1 à 200 objets `{"tool": "…", "args": {…}}`), `atomic` (défaut **true**), `path`, `page` (valeurs par défaut héritées par toutes les opérations qui ne donnent pas leur `page`).
- **Références** : toute valeur de chaîne **exactement** de la forme `"$N.chemin"` (N = index 0 de l'opération précédente, `chemin` = clés/indices séparés par des points, ex. `"$0.id"`, `"$3.ids.1"`) est remplacée par la valeur correspondante du **résultat** de l'opération N avant d'exécuter l'opération courante. Référence vers une opération future/inexistante/chemin absent → erreur qui nomme l'opération fautive. Pas d'évaluation de texte libre : seule la chaîne entière est une référence (un `text` contenant « $0.id » au milieu reste littéral).
- **Outils autorisés** : tous les outils d'édition/ajout/lecture exposés par `EDITOR_TOOLS` et les modules (y compris ceux des lots 1–4 s'ils sont présents). **Interdits dans un batch** : `batch` (pas d'imbrication), `create_board`, `save_board`, `board_changes`, `export`, `render` (effets de bord hors planche ou suivi incohérent). Message : « tool X cannot be used inside a batch ».
- **Sécurité (point dur)** : le confinement des chemins est appliqué par `call()` sur les clés **de premier niveau** seulement. Donc **toute opération contenant `path`, `output` ou `source` est refusée** (`"paths cannot be set inside a batch operation; use the batch-level path"`) — test dédié dans `security.rs`. Idem pour `data_base64` énorme : la limite d'image existante continue de s'appliquer (le test le prouve).
- **Atomicité** : `atomic: true` → à la première erreur, **toutes** les opérations déjà appliquées sont annulées sans laisser de pas d'historique, l'appel renvoie une erreur `isError` qui indique l'index et l'erreur de l'opération en cause **et** la liste des résultats déjà obtenus avant annulation (pour que l'agent comprenne), la planche et le fichier sont inchangés. `atomic: false` → s'arrête à la première erreur, **conserve** ce qui a réussi, renvoie `{"applied": k, "failed": {"index": i, "error": "…"}, "results": [...]}` en résultat normal (pas `isError`), un seul pas d'annulation pour l'ensemble.
- **Noyau** : `Editor::try_transact<R, E>(&mut self, f: impl FnOnce(&mut Self) -> Result<R, E>) -> Result<R, E>` dans `editor.rs` : exécute `f` dans une transaction ; si `Err`, **annule les opérations de cette transaction** et ne pousse rien dans l'historique ni dans la pile « rétablir » ; `revision` ne doit pas indiquer « modifié » à tort (prévoir le compteur). Tests unitaires dans `editor.rs` (modules de tests existants) : succès = un pas d'annulation ; échec = document identique et historique inchangé ; imbriqué dans `transact` extérieur (le mode direct enveloppe déjà `apply` dans `transact`) ; échec après `add_page`/`remove_page` (les opérations de pages sont aussi annulées) ; test de propriétés : séquence aléatoire d'opérations puis échec → `doc` égal à l'état initial.
- **Mode direct** : `apply` est déjà dans `ed.transact` côté application (un appel = un pas) ; `try_transact` doit fonctionner **à l'intérieur** d'une transaction ouverte (profondeur > 0) : en cas d'échec il annule seulement ses propres opérations. À vérifier par un test unitaire du cœur (pas besoin de lancer l'application).
- **Suivi `board_changes`** : le batch est un seul `apply` → `apply_tracked` l'absorbe comme changement de l'agent (test : après un batch, `board_changes` ne rapporte rien).
- Sortie normale : `{"results": [<sortie de chaque opération>], "applied": n}`.

**Tests (`tests/batch.rs`, `tests/security.rs` pour les cas de sécurité)**
Batch de 12 opérations avec références (forme → flèche → étiquette) ; référence invalide ; outil interdit ; `path`/`output`/`source` interdits ; atomique : échec en position 5 sur 8 → fichier identique octet pour octet ; non atomique : 4 conservées ; limite de 200 ; liste vide ; `page` hérité et surchargé ; résultat de `import_mermaid` réutilisable (`"$0.ids.0"`) ; `board_changes` silencieux ensuite ; très gros batch (200 ajouts) en un temps raisonnable (mesure de contrôle **sans assertion de durée**, leçon des tests flaky en CI Windows).

---

## 4. Vérifications transverses (par l'agent principal, après chaque fusion)

1. `git diff --stat feat/mcp-v2..<branche>` : seuls les fichiers autorisés du §2.3 ont bougé ; sinon demander des comptes avant de fusionner.
2. Relire le diff **de la logique** (pas du reformatage) ; lancer un sous-agent `reviewer` sur le diff de chaque lot.
3. Rejouer les portes de sortie (§2.4) sur le résultat fusionné, pas seulement sur la branche du lot.
4. Contrôle fonctionnel réel, avec le **vrai binaire** construit en debug (`cargo build -p inkboard-mcp`), pas via les tests : refaire l'essai de ce jour (Mermaid + tableau + post-it + icônes + `render` avec `show_ids` + batch), regarder l'image.
5. Taille : `inkboard.exe` release ne doit pas avoir grossi d'environ 8 Mo à cause du lot 1 (doublon du pack).

---

## 5. Risques et parades

| Risque | Parade |
|---|---|
| Conflits de fusion dans `tools.rs` | Couture de la phase 0 + ordre des vagues + fichiers autorisés |
| Pack d'icônes embarqué deux fois dans l'application | Feature `embedded` unique dans `inkboard-icons`, `library.rs` la réutilise, vérification de taille |
| Un outil vide/erreur modifie quand même le fichier | Test systématique « fichier inchangé » (déjà la convention de `tools_all.rs`) |
| Évasion du confinement via `batch` | Interdiction de `path/output/source` dans les opérations, test dédié |
| Réponse `render` trop lourde pour le client | `max_px` borné, repli automatique sous 4 Mio, `output` pour écrire un fichier |
| Régression de compatibilité des 16 outils | Sorties existantes inchangées octet pour octet (tests de référence), nouveaux champs seulement ajoutés |
| Coût de contexte des schémas (nombre d'outils) | Outils consolidés (`arrange_objects`, `manage_pages`), descriptions courtes mais précises ; 16 → 27 outils au total (§7), à mesurer à l'intégration |
| Tests flaky sur CI | Aucune assertion de durée ; données déterministes |
| Agents qui sortent du périmètre | Fichiers autorisés, relecture de diff, `git diff --stat` systématique |

---

## 6. Intégration finale (agent principal)

1. Fusionner V1 dans `feat/mcp-v2` (ordre : 1, 2, 4 ; résoudre les conflits éventuels sans réécrire), lancer V2 sur ce résultat, fusionner 3 puis 5.
2. Mettre à jour `tools_all.rs` : `TOOLS` complet (27 outils), égalité exacte restaurée ; test supplémentaire « tout outil de `tools/list` est documenté dans `docs/MCP.md` ».
3. Intégrer les `context/mcp-v2/lot-N-doc.md` dans `docs/MCP.md` **et** `docs/fr/MCP.md` (tableau des outils, convention `_image`, syntaxe `markdown`, références `$N.chemin`, limites) ; mettre à jour la phrase « 16 outils » partout (`README*`, `docs/fr/*`).
4. Ajouter dans `crates/inkboard-app/src/tests.rs` (mode direct) : un test par lot qui prouve « un appel = un pas d'annulation » et que `render`/`find_objects`/`describe_board full` ne modifient pas la révision ; test que `batch` atomique ne laisse **aucune** entrée d'historique après échec.
5. Portes de sortie complètes ; essai réel (§4.4) ; mise à jour de `context/PLAN_MCP_V2.md` avec un **journal** (ce qui est fait, ce qui n'est pas vérifié).
6. **Pas de push sur `main` sans l'accord explicite de l'utilisateur** : on présente la branche `feat/mcp-v2` (diff, tests, captures) et on demande.
7. Rappel pour l'utilisateur : le `inkboard-mcp.exe` **installé** (18/10) est ancien ; il faudra le remplacer (fermer le client MCP qui le verrouille) après publication d'une nouvelle version.

---

## 7. Inventaire des outils après le plan

Existants (16) : `create_board, save_board, board_changes, add_image, describe_board, add_shape, add_connector, add_table, add_code_block, add_formula, import_mermaid, update_object, delete_object, auto_layout, add_page, export`.
Nouveaux (12) : `search_icons, list_icon_groups, add_icon` (1) · `render` (2) · `find_objects, move_objects, arrange_objects, manage_pages, duplicate_objects` (3) · `format_text` (4) · `batch` (5). **Total 27** (le plan comptait 28 par erreur : 16 + 11).

---

## 8. Critères de réussite globaux

- Les 16 outils d'origine : sorties identiques sur une planche de référence (comparaison avant/après).
- L'essai réel du §4.4 réussit **sans** retouche manuelle : un schéma d'architecture avec icônes AWS/Azure, légendes en gras et couleur, créé par un seul `batch`, vérifié par `render` avec `show_ids`, corrigé par `update_object`/`arrange_objects`.
- `cargo fmt`, `clippy -D warnings` (stable et 1.99.1), `cargo test --workspace` verts ; aucun test ignoré de plus.
- Aucune écriture hors `--root`, aucun chemin accepté sans `resolve`.

## 9. Suites possibles (notées, non faites ici)

1. Mermaid : cylindre `[( )]` (forme « base de données ») et tracé propre des flèches de retour.
2. Mesure de texte réelle (`EguiMeasurer`) en mode direct pour `fit` et l'export.
3. Lecture/écriture de traits de stylet (au moins leur boîte et leur couleur modifiables).
4. Notifications de ressources testées avec un vrai client MCP.
5. Collaboration distante (voir la discussion du 2026-10-20) : l'agent comme participant d'une session partagée.

## 10. Journal

**2026-10-20 — exécution.** Branche `feat/mcp-v2` (base `c58b8d5`), cinq lots par agents Sonnet 5.5 en worktrees, fusionnés sans conflit (V1 : 1, 2, 4 ; V2 : 5, 3).

- **Fait** : les 11 outils nouveaux (`search_icons`, `list_icon_groups`, `add_icon`, `render`, `find_objects`, `move_objects`, `arrange_objects`, `manage_pages`, `duplicate_objects`, `format_text`, `batch`) ; `describe_board detail=full` ; `update_object` étendu ; `markdown` partout où il y a du texte ; `Editor::try_transact`. **27 outils** au total. Doc EN + FR intégrée, test `every_tool_is_documented`, test de scénario complet (`tests/scenario.rs`), trois tests du mode direct dans `inkboard-app` (un appel = un pas d'annulation, `batch` atomique sans trace, outils en lecture seule sans effet).
- **Écarts au plan** : le lot 4 a câblé `richtext` dans la couture (4 lignes) ; le lot 3 a ajouté la dépendance `regex` à `inkboard-mcp` (déjà dans `Cargo.lock`) ; le lot 5 refuse `path/output/source` à toute profondeur (plus strict que prévu) ; `render` recadre en réécrivant l'en-tête SVG (pas de changement de `svg.rs`).
- **Corrigé après l'essai réel avec le vrai binaire** : le `label` d'`add_icon` ne prenait pas `markdown` (ajouté) ; les pastilles d'identifiants de `render` masquaient le texte des objets et les étiquettes de flèches (posées au-dessus de l'objet, au quart du premier segment pour une flèche).
- **Contrôles** : `cargo fmt`, `clippy -D warnings` (stable et 1.99.1), `cargo test --workspace` (515 tests, 0 échec).
- **NON vérifié** : taille de `inkboard.exe` release (pas de doublon du pack attendu, non mesuré) ; mode direct de bout en bout avec l'application réelle et un vrai client MCP (seulement `apply` dans `transact` et le serveur d'essai) ; réponse `render` de plusieurs Mio par la socket locale en conditions réelles ; performances du parseur Markdown sur des entrées pathologiques (non bornées) ; l'étiquette de flèche courte (« HTTPS ») est barrée par sa propre flèche dans le rendu : défaut du moteur de rendu, hors lot.
- **À faire par l'utilisateur** : décider du push de `feat/mcp-v2` (pas de push sans accord) ; remplacer le `inkboard-mcp.exe` installé (18/10) après publication.
