# Tableau blanc

Tableau blanc infini, **100 % hors ligne**, écrit en Rust. Il remplace l'application de bureau Microsoft Whiteboard et vise
les manques les plus souvent reprochés aux outils du genre (Miro, Excalidraw, draw.io, Whiteboard).

> État : **jalon 2 (texte riche et raccourcis personnalisables) sur le socle du jalon 1 (moteur + formats + application native)**. Le prototype web d'origine est conservé dans
> [`prototype-web/`](prototype-web/) et reste utilisable ; ses fichiers `.tableau.json` s'importent dans la version Rust.

## Démarrer

```bash
cargo run --release -p wb-app                 # l'application
cargo run --release -p wb-app -- schema.drawio # ouvrir un fichier (draw.io, Excalidraw, JSON Canvas, Mermaid, ancien format)
cargo test --workspace                        # 160 tests
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
| [`wb-cli`](crates/wb-cli) | `wb convert`, `wb info`, `wb bench`. |
| [`wb-app`](crates/wb-app) | Application de bureau (egui) : canvas, outils, panneau de style, pages, calques, texte riche, raccourcis personnalisables, dialogues. |

## Texte riche et raccourcis

- **Texte riche** : double-clic sur une forme, un tableau ou une flèche, puis sélection de mots (souris ou Maj+flèches) →
  gras (Ctrl+B), italique (Ctrl+I), souligné (Ctrl+U), barré, **couleur** et **surlignage** par mot, via la barre flottante
  ou le panneau de droite. Sans sélection, la mise en forme s'applique à tout le texte ; hors édition, à tout le texte des
  objets sélectionnés. La frappe continue dans le style du caractère précédent.
- **Raccourcis personnalisables** : menu *Affichage → Raccourcis clavier…* (39 commandes, plusieurs raccourcis par commande,
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
| SVG, PNG | – | ✔ |

## Performances (mesurées)

`tableau-blanc --bench fichier` mesure le temps CPU de construction d'une image en vue « tout afficher » avec zoom et défilement
continus (build release). Plateau synthétique `wb bench N` : 60 % formes avec texte, 25 % traits, 15 % connecteurs liés.

| Objets | Avant optimisation | Après |
|---|---|---|
| 2 000 | 2,9 ms | 0,8 ms |
| 20 000 | 199 ms | 5,1 ms |
| 100 000 | > 2 min (inutilisable) | 21 ms |

Le coût dominant restant est le parcours linéaire de tous les objets à chaque image ; un index spatial est prévu
([feuille de route](docs/ROADMAP.md)).

## Documentation

- [Architecture et décisions](docs/ARCHITECTURE.md)
- [Feuille de route et état des « plaintes » des utilisateurs](docs/ROADMAP.md)
