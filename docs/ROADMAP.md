# Feuille de route

## Plaintes d'utilisateurs → état

Recherche faite en octobre 2026 (avis G2/Capterra/GetApp, tickets GitHub d'Excalidraw, fils Hacker News).

| Plainte | Outils | État |
|---|---|---|
| Lag sur grands tableaux | Miro, Whiteboard, Excalidraw | ✔ mesuré : 100 000 objets en 4,5 ms/image (index spatial, niveau de détail) |
| Connecteurs fragiles / qui ne suivent pas | draw.io, Excalidraw | ✔ connecteurs **liés**, droits/coudés/courbes, tests |
| Pas de mise en page automatique | Excalidraw | ✔ commande « mise en page automatique » + import Mermaid |
| Tableaux de cellules | Excalidraw (n°1 par commentaires) | ✔ insertion, édition des cellules, lignes/colonnes ; mise en forme à venir |
| Calques | Excalidraw, Whiteboard | ✔ visibilité, verrouillage, déplacement de la sélection |
| Plusieurs pages / onglets | Excalidraw | ✔ |
| Arrondi réglable des coins, surligneur, grille type cahier | Excalidraw | ✔ |
| Export lisible dans d'autres outils | Whiteboard, Miro | ✔ draw.io, Excalidraw, JSON Canvas, SVG, PNG |
| Hors ligne, sans compte, sans limite de tableaux | Miro | ✔ par construction |
| Colorer / surligner des mots d'un paragraphe | Excalidraw | ✔ sélection de mots, gras/italique/souligné/barré, couleur, surlignage (jalon 2) |
| Raccourcis personnalisables | Excalidraw | ✔ 40 commandes, plusieurs raccourcis, fichier JSON, conflits gérés (jalon 2) |
| Stylet : pression | Excalidraw | ✔ largeur variable (à valider sur du matériel réel) |
| Stylet : gomme au bouton | Excalidraw | ✘ non exposée par egui |
| Blocs de code | Excalidraw | ✔ (sans coloration syntaxique) |
| LaTeX | Excalidraw | ◐ aperçu Unicode approximatif (pas de composition) |
| Présentation, minuteur, vote | Excalidraw | ✔ plein écran page par page, minuteur, pastilles |
| Connecteurs qui évitent les formes | draw.io | ✔ routage A*, points de passage |
| Presse-papiers (images, texte) | tous | ✔ |
| Export PDF | Whiteboard | ✔ vectoriel |

## Reste à faire

- **`wb-mcp`** : serveur MCP en Rust sur `wb-core` (créer/modifier des objets, importer Mermaid, exporter) ; niveau A sur fichiers,
  niveau B en direct avec l'application ouverte. *Volontairement repoussé.*
- Installateur Windows **signé** (nécessite un certificat de signature de code ; les scripts `scripts/` fournissent l'archive portable et une installation par utilisateur).
- Coloration syntaxique des blocs de code ; vraie composition LaTeX.
- Gomme au bouton du stylet (dépend d'egui/winit) ; validation de la pression sur du matériel réel.
- Alignement par ligne dans le champ de saisie pendant l'édition d'un texte centré multiligne.
- Version WASM du moteur (même code) si un usage web est souhaité.

## Limites connues

- Pas de tests visuels automatisés (la capture d'écran `--screenshot` sert au contrôle manuel).
- Les traits au crayon sont exportés en images vers draw.io (pas d'équivalent éditable).
- Les formats d'échange n'ont été confrontés qu'à leurs propres exports et à des fichiers écrits à la main, à l'exception de
  l'export draw.io vérifié dans le visualiseur officiel ; aucun vrai fichier Excalidraw/Obsidian n'a été testé.
- Le workflow CI (`.github/workflows/ci.yml`) n'a jamais été exécuté.
- Le routage coudé avec contournement est désactivé au-delà de 5 000 objets (coût linéaire par connecteur).
- La formule LaTeX est figée en texte Unicode : le source n'est pas conservé.
