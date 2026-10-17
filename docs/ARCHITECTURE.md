# Architecture

## Principes

1. **Le moteur ne connaît pas l'interface.** `wb-core` n'a ni fenêtre, ni police, ni dépendance graphique. Tout ce qui peut
   se tester sans écran se teste dans `wb-core` / `wb-formats` ; l'interface (`wb-app`) est une couche mince au-dessus.
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

## Interface (`wb-app`)

- `render.rs` : dessine une page (formes, texte riche, traits lissés à largeur variable, connecteurs et pointes, images, tableaux). Niveau de détail :
  sous 3 px un objet devient un point ; les caches de l'application (`Caches` : identifiant → position, index spatial) servent au rendu et aux sélections.
- `tools.rs` : machine à états des gestes (`Drag`), poignées, création d'objets, édition de texte.
  Les calculs de poignées (redimensionner un cadre tourné, rotation, magnétisme) vivent dans `wb-core::transform` et sont testés à part.
- `rich_edit.rs` : édition de texte riche. Le champ de saisie egui garde la chaîne brute ; `RichText` suit chaque frappe
  (`apply_edit` : préfixe/suffixe communs) et un `layouter` affiche les styles. La barre flottante et les raccourcis appellent
  `RichText::format` (moteur). La sélection est mémorisée puis rétablie après un clic sur la barre.
- `keymap.rs` : `Action` (40 commandes) → `Shortcut`s, remplacement en cas de conflit, combinaisons réservées, fichier JSON tolérant.
  `App::run_action` est le point d'entrée unique des commandes (raccourcis, menus).
- `ui.rs` : barres, panneau de style, calques, dialogues (dont réglage des raccourcis). `app.rs` : état, fichiers, sauvegarde automatique.
- `clipboard.rs` : presse-papiers (objets internes, images et texte du système). `insert.rs` : bloc de code, formule, pastille de vote.
  Mode présentation (`App::presenting`, `present_overlay`) et minuteur dans `app.rs` / `ui.rs`.
- `tests.rs` : 64 tests qui pilotent l'application avec une vraie souris/clavier simulés (`egui_kittest`), sans fenêtre.

## Modules ajoutés au noyau (`wb-core`)

- `spatial.rs` : index spatial en grille (`SpatialIndex`), table id → position, accès rapide aux objets par identifiant.
- `route.rs` : routage orthogonal A* sur la grille formée par les bords des obstacles (pénalité de coude) ; `connector.rs` l'utilise pour les
  connecteurs coudés (`avoid_obstacles`) uniquement quand le chemin simple traverse une forme. Désactivé au-delà de 5 000 objets.
- `pen.rs` : lissage des traits, largeur selon la pression, contour polygonal du trait.
- `latex.rs` : conversion LaTeX → Unicode (approximation, pas un moteur de composition).

### Pourquoi egui ?

Un seul langage et un exécutable natif, hors ligne par construction, avec une API de dessin qui permet de maîtriser le rendu du
tableau. Limites connues, assumées pour ce jalon : pas d'icônes système (icônes dessinées à la main), éditeur de texte basique
accessibilité limitée. Si le rendu devait dépasser egui
(très grands tableaux, effets), `wb-core` reste indépendant : on pourrait changer de moteur de rendu sans toucher au reste.

## Tests

| Crate | Tests | Ce qui est couvert |
|---|---|---|
| `wb-core` | 102 | géométrie, modèle, connecteurs (rotation, coude, courbe), sélection, historique, sauvegarde, SVG, mise en page, poignées, texte riche (insertion/suppression/bascule de styles), routage avec obstacles, index spatial, pression du stylet, LaTeX |
| `wb-formats` | 38 | aller-retours draw.io / Excalidraw / JSON Canvas, imports étrangers, draw.io compressé, Mermaid (flowchart, état, séquence), PNG, PDF |
| `wb-app` | 64 | création, déplacement avec flèches liées, redimensionnement, rotation, lasso, gomme, annulation d'un geste, texte, tableaux, import, zoom ; **texte riche** (frappe, sélection au clavier, gras/couleur/surlignage) ; **raccourcis** (réaffectation, retrait, enregistrement d'une touche, persistance, raccourcis en édition) ; **points de passage**, contournement d'obstacles, bloc de code, formule, vote, présentation, minuteur, presse-papiers, stylet |

En plus : l'export draw.io a été ouvert dans le visualiseur officiel diagrams.net (formes, flèches liées, pointillés, rotation).
