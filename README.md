# Tableau blanc

Tableau blanc infini, **100 % hors ligne**, écrit en Rust. Il remplace l'application de bureau Microsoft Whiteboard et vise
les manques les plus souvent reprochés aux outils du genre (Miro, Excalidraw, draw.io, Whiteboard).

> État : **application native complète** (moteur, formats d'échange, texte riche, raccourcis, stylet, grands tableaux, présentation) ; le serveur MCP pour agents est volontairement repoussé. Le prototype web d'origine est conservé dans
> [`prototype-web/`](prototype-web/) et reste utilisable ; ses fichiers `.tableau.json` s'importent dans la version Rust.

## Démarrer

```bash
cargo run --release -p wb-app                 # l'application
cargo run --release -p wb-app -- schema.drawio # ouvrir un fichier (draw.io, Excalidraw, JSON Canvas, Mermaid, ancien format)
cargo test --workspace                        # 216 tests
powershell -File scripts/package.ps1          # archive portable Windows (dist/)
```

CLI de conversion (utile aux scripts et, plus tard, aux agents) :

```bash
cargo run --release -p wb-cli -- convert schema.mmd schema.drawio      # Mermaid → draw.io
cargo run --release -p wb-cli -- convert ancien.tableau.json nouveau.png
cargo run --release -p wb-cli -- info schema.drawio
```

## Contenu

| Crate | Rôle |
|---|---|
| [`wb-core`](crates/wb-core) | Modèle (pages, calques, objets), géométrie, **connecteurs liés**, historique, export SVG, mise en page automatique. Aucune dépendance à une interface. |
| [`wb-formats`](crates/wb-formats) | Import/export draw.io, Excalidraw, JSON Canvas ; import Mermaid ; ancien format du prototype ; rendu PNG. |
| [`wb-pen`](crates/wb-pen) | Seul crate contenant du `unsafe` : lecture de l'état de la gomme du stylet (messages `WM_POINTER*` de Windows). |
| [`wb-cli`](crates/wb-cli) | `wb convert`, `wb info`, `wb bench`. |
| [`wb-app`](crates/wb-app) | Application de bureau (egui) : canvas, outils, panneau de style, pages, calques, texte riche, raccourcis personnalisables, dialogues. |

## Fonctions notables

- **Connecteurs liés** (droits, coudés, courbes) qui suivent les formes ; les coudés **contournent les formes** rencontrées ; poignées de
  **points de passage** (glisser le rond creux au milieu d'un tronçon pour en créer un, Alt+clic pour le retirer, menu « Redresser »).
- **Stylet** : la pression fait varier l'épaisseur du trait (écran, SVG, Excalidraw) ; l'**extrémité gomme** du stylet efface, quel que soit l'outil actif (Windows, via l'API Pointer — voir `wb-pen`).
- **Presse-papiers système** : copier/coller d'objets, d'images et de texte.
- **Menu Insérer** : bloc de code (police à chasse fixe, coloration syntaxique générique), **formule LaTeX composée** (vraie typographie TeX, police STIX Two Math embarquée : fractions, racines, sommes, intégrales, matrices ; double-clic pour la modifier, couleur = couleur de trait, source conservée),
  pastilles de vote (outil dédié).
- **Présentation** (F5) : plein écran sans interface, flèches / espace / Page↓ pour changer de page, Échap pour quitter ; **minuteur** (menu Affichage).
- **Exports** : draw.io, Excalidraw, JSON Canvas, SVG, PNG, **PDF vectoriel**.
- **Distribution** : icône, archive portable et installation par utilisateur (voir [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)).

## Texte riche et raccourcis

- **Texte riche** : double-clic sur une forme, un tableau ou une flèche, puis sélection de mots (souris ou Maj+flèches) →
  gras (Ctrl+B), italique (Ctrl+I), souligné (Ctrl+U), barré, **couleur** et **surlignage** par mot, via la barre flottante
  ou le panneau de droite. Sans sélection, la mise en forme s'applique à tout le texte ; hors édition, à tout le texte des
  objets sélectionnés. La frappe continue dans le style du caractère précédent.
- **Raccourcis personnalisables** : menu *Affichage → Raccourcis clavier…* (40 commandes, plusieurs raccourcis par commande,
  recherche, réinitialisation par commande ou globale, remplacement automatique en cas de conflit). Réglages dans
  `%APPDATA%\TableauBlanc\keymap.json` (modifiable à la main ; les entrées invalides sont ignorées et signalées).
  Réservés : Échap, flèches (déplacement fin), Ctrl+C/X/V.

## Formats

| Format | Ouvrir | Enregistrer / exporter |
|---|---|---|
| Natif `.tableau.json` (v2, versionné) | ✔ | ✔ (enregistrement atomique + sauvegarde automatique) |
| Prototype web `.tableau.json` (v1) | ✔ | – |
| draw.io `.drawio` (compressé ou non, multi-pages) | ✔ | ✔ |
| Excalidraw `.excalidraw` | ✔ | ✔ |
| JSON Canvas `.canvas` | ✔ | ✔ (formes avec texte et flèches entre formes) |
| Mermaid (`flowchart`, `stateDiagram`, `sequenceDiagram`) | ✔ | – |
| SVG, PNG, PDF | – | ✔ |

## Performances (mesurées)

`tableau-blanc --bench fichier` mesure le temps CPU de construction d'une image en vue « tout afficher » avec zoom et défilement
continus (build release). Plateau synthétique `wb bench N` : 60 % formes avec texte, 25 % traits, 15 % connecteurs liés.

| Objets | Avant optimisation | Après (vue « tout afficher ») | Après (zoom 100 %) |
|---|---|---|---|
| 2 000 | 2,9 ms | 0,8 ms | 0,3 ms |
| 20 000 | 199 ms | 3,3 ms | 0,3 ms |
| 100 000 | > 2 min (inutilisable) | 4,5 ms | 0,3 ms |

Moyens : table des identifiants et **index spatial en grille** reconstruits une fois par modification (28 ms pour 100 000 objets),
niveau de détail (objets de moins de 3 px dessinés comme des points), sélection et survol par l'index (< 0,01 ms).
L'index n'est utilisé qu'à partir de 400 objets et suspendu pendant les gestes qui déplacent des objets.

## Documentation

- [Architecture et décisions](docs/ARCHITECTURE.md)
- [Feuille de route et état des « plaintes » des utilisateurs](docs/ROADMAP.md)
