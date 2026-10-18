# Architecture

## Principes

1. **Le moteur ne connaît pas l'interface.** `inkboard-core` n'a ni fenêtre, ni police, ni dépendance graphique. Tout ce qui peut
   se tester sans écran se teste dans `inkboard-core` / `inkboard-formats` ; l'interface (`inkboard-app`) est une couche mince au-dessus.
   Le même moteur servira le futur serveur MCP, la CLI et d'éventuels autres fronts (WASM).
2. **Les connecteurs ne stockent pas leur tracé.** Un connecteur garde ses extrémités (`Free(point)` ou
   `Bound{cible, ancre}`) ; le chemin est **recalculé à la demande** depuis l'état courant de la page
   (`Page::connector_path`). Déplacer, redimensionner ou tourner une forme fait donc suivre ses flèches, sans aucune mise à jour
   et sans état périmé possible. Supprimer une forme détache ses connecteurs dans la même transaction.
3. **Toute modification passe par l'`Editor`.** Les opérations (`Insert`, `Remove`, `Replace`, `Reorder`, `SetLayers`, pages, fond)
   sont réversibles ; une transaction = une étape d'annulation. Pendant un geste (déplacement, redimensionnement…), l'interface
   modifie la page « en direct » sans historique puis enregistre **un seul** avant/après au relâchement.
4. **Le texte est mesuré par un `Measurer` injecté.** L'interface fournit les vraies polices (egui) ; la CLI utilise une
   mesure approchée. La mise en page du texte (`layout_rich`) est donc identique pour l'écran, le SVG et les formats d'échange.
5. **Texte riche dans le modèle dès le départ** : `RichText` = fragments stylés (gras, italique, souligné, barré, couleur,
   surlignage). Les couleurs de mots et le surlignage sont déjà rendus (écran, SVG, draw.io).
6. **Formats d'échange = fonctions pures** `contenu → Document` et `Document → contenu`, testées par aller-retours.
   Un import renvoie un `Report` (éléments approximés / ignorés) plutôt que d'échouer en silence.

## Modèle

```
Document { pages[], fond, ressources (images, dédupliquées) }
Page     { calques[], objets[] (ordre = z), vue }
Object   { id, style, groupe, verrou, calque, kind }
Kind     = Box (rect/ellipse/losange/triangle/note/texte) | Pen | Connector | Image | Table
```

Les traits (`Pen`) stockent leurs points en coordonnées monde dans un `Arc<Vec<_>>` (copie bon marché pour l'historique).

## Interface (`inkboard-app`)

- `render.rs` : dessine une page (formes, texte riche, traits lissés à largeur variable, connecteurs et pointes, images, tableaux). Niveau de détail :
  sous 3 px un objet devient un point ; les caches de l'application (`Caches` : identifiant → position, index spatial) servent au rendu et aux sélections.
- `tools.rs` : machine à états des gestes (`Drag`), poignées, création d'objets, édition de texte.
  Les calculs de poignées (redimensionner un cadre tourné, rotation, magnétisme) vivent dans `inkboard-core::transform` et sont testés à part.
- `rich_edit.rs` : édition de texte riche. Le champ de saisie egui garde la chaîne brute ; `RichText` suit chaque frappe
  (`apply_edit` : préfixe/suffixe communs) et un `layouter` affiche les styles. La barre flottante et les raccourcis appellent
  `RichText::format` (moteur). La sélection est mémorisée puis rétablie après un clic sur la barre.
- `keymap.rs` : `Action` (41 commandes) → `Shortcut`s, remplacement en cas de conflit, combinaisons réservées, fichier JSON tolérant.
  `App::run_action` est le point d'entrée unique des commandes (raccourcis, menus).
- `theme.rs` : palette claire/sombre du prototype, styles egui (arrondis, ombres, tailles de texte) et widgets partagés (boutons à icône, lignes de menu, boutons segmentés, cartes flottantes) ;
  `icons.rs` : jeu d'icônes SVG du prototype, rastérisé une fois (resvg, 3×) en texture blanche puis teinté ; `chrome.rs` : les cartes (titre + menu, outils, pages, zoom, aide), le menu principal et les menus contextuels ;
  `panel.rs` : panneau de style flottant (pastilles, boutons segmentés, ordre, alignement, calques) ; `ui.rs` : présentation, dialogues centrés sur fond assombri, notifications.
  Le canevas occupe toute la fenêtre ; les cartes sont des `egui::Area` au-dessus (l'aire d'une carte nouvellement apparue est trop large pendant ses toutes premières images : les tests laissent le panneau se stabiliser avec `T::tool`). `app.rs` : état, fichiers, sauvegarde automatique.
- `clipboard.rs` : presse-papiers (objets internes, images et texte du système). `insert.rs` : bloc de code, formule, pastille de vote.
  Mode présentation (`App::presenting`, `present_overlay`) et minuteur dans `app.rs` / `ui.rs`.
- `tests.rs` : 78 tests qui pilotent l'application avec une vraie souris/clavier simulés (`egui_kittest`), sans fenêtre.

## Modules ajoutés au noyau (`inkboard-core`)

- `spatial.rs` : index spatial en grille (`SpatialIndex`), table id → position, accès rapide aux objets par identifiant.
- `route.rs` : routage orthogonal A* sur la grille formée par les bords des obstacles (pénalité de coude) ; `connector.rs` l'utilise pour les
  connecteurs coudés (`avoid_obstacles`) uniquement quand le chemin simple traverse une forme. Désactivé au-delà de 5 000 objets.
- `pen.rs` : lissage des traits, largeur selon la pression, contour polygonal du trait.
- `highlight.rs` : coloration syntaxique générique des blocs de code (`RichText` coloré).

`inkboard-formats::math` compose les formules LaTeX en SVG (crate `latex-rust`, police STIX Two Math embarquée). Une formule est un `ImageObj` dont l'asset est ce SVG
**dessiné en blanc** et dont `latex` conserve le source : l'écran teinte la texture avec la couleur de trait, l'export SVG remplace le blanc par cette couleur.

`inkboard-mcp` : serveur MCP (JSON-RPC sur stdio, écrit à la main) au-dessus de `inkboard-core`/`inkboard-formats` ; chaque outil est une fonction `apply(&mut Editor, nom, args)` ; le mode fichier ouvre le fichier, l'applique et réenregistre atomiquement, le mode direct la fait exécuter par l'application (`inkboard-app::live` : serveur local 127.0.0.1 + jeton, exécution sur le fil d'interface dans une transaction).
`inkboard-app::watch` recharge le fichier ouvert quand un autre programme le modifie (sauf modifications locales non enregistrées). Détails : [MCP.md](MCP.md).

`inkboard-mcp::resources` : ressources MCP et fil de surveillance (sondage 600 ms → `notifications/resources/updated`) ; `tools::apply_tracked` : suivi des changements de l'utilisateur (empreinte par objet, les modifications de l'agent sont absorbées dans la référence).

`inkboard-pen` (Windows) installe un *subclass* de fenêtre qui lit les `WM_POINTER*` sans les consommer et expose `eraser_active()` et `last_sample()` (pression, indicateurs, inclinaison ; utilisés par le diagnostic `--pen-test`, `inkboard-app::pen_test`) ; l'application remplace alors l'outil par la gomme
pour le geste (`App::eraser_probe`, substituable dans les tests). C'est le seul crate qui autorise `unsafe`.

### Pourquoi egui ?

Un seul langage et un exécutable natif, hors ligne par construction, avec une API de dessin qui permet de maîtriser le rendu du
tableau. Limites connues, assumées pour ce jalon : pas d'icônes système (icônes dessinées à la main), éditeur de texte basique
accessibilité limitée. Si le rendu devait dépasser egui
(très grands tableaux, effets), `inkboard-core` reste indépendant : on pourrait changer de moteur de rendu sans toucher au reste.

## Tests

| Crate | Tests | Ce qui est couvert |
|---|---|---|
| `inkboard-core` | 101 | géométrie, modèle, connecteurs (rotation, coude, courbe), sélection, historique, sauvegarde, SVG, mise en page, poignées, texte riche (insertion/suppression/bascule de styles), routage avec obstacles, index spatial, pression du stylet, coloration |
| `inkboard-formats` | 46 | aller-retours draw.io / Excalidraw / JSON Canvas, imports étrangers, draw.io compressé, Mermaid (flowchart, état, séquence), PNG, PDF, composition LaTeX, fichiers Excalidraw/Canvas réalistes |
| `inkboard-mcp` | 8 | session complète sur le vrai binaire (poignée de main, outils, confinement à la racine, exports) ; le mode direct, les ressources/notifications et `board_changes` en direct sont testés dans `inkboard-app` (5 tests : édition annulable, ciblage du fichier ouvert, jeton, ressources et notifications, changements et images) |
| `inkboard-app` | 85 | création, déplacement avec flèches liées, redimensionnement, rotation, lasso, gomme, annulation d'un geste, texte, tableaux, import, zoom ; **texte riche** (frappe, sélection au clavier, gras/couleur/surlignage) ; **raccourcis** (réaffectation, retrait, enregistrement d'une touche, persistance, raccourcis en édition) ; **points de passage**, contournement d'obstacles, bloc de code (coloré), formule composée (insertion, modification, export teinté), vote, gomme du stylet, présentation, minuteur, presse-papiers, stylet |

En plus : l'export draw.io a été ouvert dans le visualiseur officiel diagrams.net (formes, flèches liées, pointillés, rotation).
