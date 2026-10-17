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
| Raccourcis personnalisables | Excalidraw | ✔ 41 commandes, plusieurs raccourcis, fichier JSON, conflits gérés (jalon 2) |
| Stylet : pression | Excalidraw | ◐ largeur variable ; **non validé sur du matériel réel** (mode `--pen-test`) |
| Stylet : gomme (extrémité) | Excalidraw | ◐ implémenté via l'API Pointer de Windows ; **non validé sur du matériel réel** (mode `--pen-test` pour le valider : docs/STYLET.md) |
| Blocs de code | Excalidraw | ✔ coloration syntaxique minimale (commentaires, chaînes, nombres, mots-clés ; menu « Colorer le code ») |
| LaTeX | Excalidraw | ✔ composition TeX (STIX Two Math), formule modifiable, couleur suivie |
| Présentation, minuteur, vote | Excalidraw | ✔ plein écran page par page, minuteur, pastilles, pointeur laser |
| Interface soignée (cartes flottantes, thème sombre) | – | ✔ identique au prototype web |
| Connecteurs qui évitent les formes | draw.io | ✔ routage A*, points de passage |
| Presse-papiers (images, texte) | tous | ✔ |
| Pilotage par des agents (MCP) | – | ✔ `wb-mcp` : fichiers et mode direct (application ouverte, annulable, sélection visible) |
| Export PDF | Whiteboard | ✔ vectoriel |

## Reste à faire

- `wb-mcp` : vérifier ce que les clients réels (Claude Code, Claude Desktop) font des notifications de ressources ; notifications avec le détail du changement ; lecture du contenu des images.
- Installateur Windows **signé** (nécessite un certificat de signature de code ; les scripts `scripts/` fournissent l'archive portable et une installation par utilisateur).
- Coloration par langage (la coloration actuelle est générique) ; texte (`\text`) et environnements avancés de la bibliothèque de composition au-delà de ses capacités.
- **Validation de la pression et de la gomme du stylet sur du matériel réel** (procédure : docs/STYLET.md) ; gomme sous macOS/Linux (non gérée).
- Alignement par ligne dans le champ de saisie pendant l'édition d'un texte centré multiligne.
- Version WASM du moteur (même code) si un usage web est souhaité.

## Limites connues

- Pas de tests visuels automatisés (la capture d'écran `--screenshot` sert au contrôle manuel).
- Les traits au crayon sont exportés en images vers draw.io (pas d'équivalent éditable).
- Les formats d'échange n'ont été confrontés qu'à leurs propres exports et à des fichiers écrits à la main, à l'exception de
  l'export draw.io vérifié dans le visualiseur officiel ; des fichiers Excalidraw et Obsidian Canvas écrits comme les vrais outils (`wb-formats/tests/realistic.rs`) s'importent correctement, mais aucun fichier réel d'utilisateur n'a été testé.
- Le workflow CI (`.github/workflows/ci.yml`, Windows + Linux + macOS) n'a jamais été exécuté. Linux et macOS : le projet compile (`cargo check` en cross-compilation) mais n'a jamais été lancé ni testé ; paquets et installateurs : à faire.
- Le routage coudé avec contournement est désactivé au-delà de 5 000 objets (coût linéaire par connecteur).
- Les formules dépendent du crate `latex-rust` 2.1.1 (récent, peu éprouvé ; la 3.x exige Rust 1.99) : une commande non gérée est signalée dans la fenêtre de saisie. Une formule est rastérisée à l'écran (×4) mais reste vectorielle dans les exports SVG et PDF.
