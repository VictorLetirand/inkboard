# Feuille de route

## Plaintes d'utilisateurs → état

Recherche faite en octobre 2026 (avis G2/Capterra/GetApp, tickets GitHub d'Excalidraw, fils Hacker News).

| Plainte | Outils | État |
|---|---|---|
| Lag sur grands tableaux | Miro, Whiteboard, Excalidraw | ✔ mesuré : 100 000 objets en 21 ms/image ; index spatial prévu |
| Connecteurs fragiles / qui ne suivent pas | draw.io, Excalidraw | ✔ connecteurs **liés**, droits/coudés/courbes, tests |
| Pas de mise en page automatique | Excalidraw | ✔ commande « mise en page automatique » + import Mermaid |
| Tableaux de cellules | Excalidraw (n°1 par commentaires) | ✔ insertion, édition des cellules, lignes/colonnes ; mise en forme à venir |
| Calques | Excalidraw, Whiteboard | ✔ visibilité, verrouillage, déplacement de la sélection |
| Plusieurs pages / onglets | Excalidraw | ✔ |
| Arrondi réglable des coins, surligneur, grille type cahier | Excalidraw | ✔ |
| Export lisible dans d'autres outils | Whiteboard, Miro | ✔ draw.io, Excalidraw, JSON Canvas, SVG, PNG |
| Hors ligne, sans compte, sans limite de tableaux | Miro | ✔ par construction |
| Colorer / surligner des mots d'un paragraphe | Excalidraw | ✔ sélection de mots, gras/italique/souligné/barré, couleur, surlignage (jalon 2) |
| Raccourcis personnalisables | Excalidraw | ✔ 39 commandes, plusieurs raccourcis, fichier JSON, conflits gérés (jalon 2) |
| Stylet : gomme au bouton, pression | Excalidraw | ✘ jalon 2 (dépend du support egui/winit) |
| LaTeX, blocs de code | Excalidraw | ✘ jalon 3 |
| Présentation, minuteur, vote | Excalidraw | ✘ jalon 3 |

## Jalons

**Jalon 2 — finitions de l'édition** (texte riche et raccourcis : ✔ faits)
- Reste : menu contextuel complet ; copier/coller d'images système ; mise en forme riche dans les cellules de tableau depuis le panneau ;
  alignement par ligne dans le champ de saisie (pendant l'édition, un texte centré s'affiche centré en bloc).
- Index spatial (R-tree) pour le culling et la sélection au-delà de 100 000 objets ; cache de mise en forme du texte.
- Pression du stylet, gomme du stylet ; mode tablette.
- Routage coudé qui évite les formes ; poignées de points de passage.
- Installateur Windows signé, icône, associations de fichiers (`.tableau.json`, `.drawio`).

**Jalon 3 — agents et extras**
- `wb-mcp` : serveur MCP en Rust sur `wb-core` (créer/modifier des objets, importer Mermaid, exporter) ; niveau A sur fichiers,
  niveau B en direct avec l'application ouverte.
- LaTeX, blocs de code, mode présentation, minuteur.
- Version WASM du moteur (même code) si un usage web est souhaité.

## Limites connues (jalon 1)

- Pas de tests visuels automatisés (la capture d'écran `--screenshot` sert au contrôle manuel).
- Les traits au crayon sont exportés en images vers draw.io (pas d'équivalent éditable).
- Les formats d'échange n'ont été confrontés qu'à leurs propres exports et à des fichiers écrits à la main, à l'exception de
  l'export draw.io vérifié dans le visualiseur officiel.
