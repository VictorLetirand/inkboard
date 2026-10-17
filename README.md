# Tableau blanc

Tableau blanc infini, **100 % hors ligne**, écrit en Rust. Il remplace l'application de bureau Microsoft Whiteboard et vise
les manques les plus souvent reprochés aux outils du genre (Miro, Excalidraw, draw.io, Whiteboard).

> État : **application native complète** (moteur, formats d'échange, texte riche, raccourcis, stylet, grands tableaux, présentation)  et **serveur MCP** pour piloter le tableau depuis un agent (niveau fichiers). Le prototype web d'origine est conservé dans
> [`prototype-web/`](prototype-web/) et reste utilisable ; ses fichiers `.tableau.json` s'importent dans la version Rust.

## Installer

Des installeurs (Windows `.exe` / `.msi`, Linux `.deb` / `.AppImage`, macOS `.dmg`) sont publiés sur la page *Releases* du dépôt à chaque version (voir [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)) ;
ils ne sont pas encore signés (avertissement SmartScreen / Gatekeeper au premier lancement). Seuls les installeurs Windows ont été construits et testés à ce jour.

## Démarrer

```bash
cargo run --release -p wb-app                 # l'application
cargo run --release -p wb-app -- schema.drawio # ouvrir un fichier (draw.io, Excalidraw, JSON Canvas, Mermaid, ancien format)
cargo test --workspace                        # 245 tests
cargo packager --release -c Packager.toml    # installeurs (voir docs/DISTRIBUTION.md)
```

CLI de conversion (utile aux scripts et, plus tard, aux agents) :

```bash
cargo run --release -p wb-cli -- convert schema.mmd schema.drawio      # Mermaid → draw.io
cargo run --release -p wb-cli -- convert ancien.tableau.json nouveau.png
cargo run --release -p wb-cli -- info schema.drawio
```

## Plateformes

| | Windows | Linux | macOS |
|---|---|---|---|
| Compilation de tout le projet | ✔ construit, testé, utilisé | ✔ `cargo check` réussi (cross-compilation depuis Windows) | ✔ `cargo check` réussi (idem, `aarch64-apple-darwin`) |
| Exécution / tests | ✔ | **jamais lancé** | **jamais lancé** |
| Gomme du stylet (`wb-pen`) | ✔ (non validée sur matériel) | – (sans effet) | – (sans effet) |
| Icône de l'exécutable, archive portable, installation par utilisateur | ✔ (`scripts/*.ps1`) | – | – |
| Dossier des données (autosave, raccourcis, mode direct MCP) | `%APPDATA%\TableauBlanc` | `$XDG_DATA_HOME` ou `~/.local/share/TableauBlanc` | `~/Library/Application Support/TableauBlanc` |

Le code est portable (Rust + egui, pas d'API propre à un système en dehors de `wb-pen`), mais seul Windows a été exécuté. Sous Linux et macOS : polices système cherchées dans les dossiers habituels
(à défaut, les polices intégrées d'egui : Manuscrit et Serif y ressemblent à Sans), raccourcis avec Cmd à la place de Ctrl sous macOS, pas d'installateur ni de paquet. Le workflow CI
(`.github/workflows/ci.yml`) teste les trois systèmes mais n'a jamais été exécuté.

## Contenu

| Crate | Rôle |
|---|---|
| [`wb-core`](crates/wb-core) | Modèle (pages, calques, objets), géométrie, **connecteurs liés**, historique, export SVG, mise en page automatique. Aucune dépendance à une interface. |
| [`wb-formats`](crates/wb-formats) | Import/export draw.io, Excalidraw, JSON Canvas ; import Mermaid ; ancien format du prototype ; rendu PNG. |
| [`wb-mcp`](crates/wb-mcp) | Serveur MCP (stdio) : un agent crée/modifie des tableaux, importe du Mermaid, exporte. Voir [docs/MCP.md](docs/MCP.md). |
| [`wb-pen`](crates/wb-pen) | Seul crate contenant du `unsafe` : lecture de l'état de la gomme du stylet (messages `WM_POINTER*` de Windows). |
| [`wb-cli`](crates/wb-cli) | `wb convert`, `wb info`, `wb bench`. |
| [`wb-app`](crates/wb-app) | Application de bureau (egui) : canvas, outils, panneau de style, pages, calques, texte riche, raccourcis personnalisables, dialogues. |

## Interface

L'interface reprend celle du prototype web : un canevas plein écran et des **cartes flottantes** (blanches, bordées, ombrées, coins de 10 px) — titre et menu en haut à gauche,
outils au centre en haut, réglages de style à gauche (visibles seulement quand il y a quelque chose à régler), annuler / zoom en bas à gauche, pages au centre en bas, aide en bas à droite.
Mêmes icônes (jeu SVG du prototype), mêmes couleurs, mêmes boutons segmentés, menu principal à entrées contiguës avec raccourcis, thème **clair et sombre** qui suit le fond du tableau.

![Interface](docs/img/interface.png)

| Menu principal | Thème sombre et réglages de style |
|---|---|
| ![Menu](docs/img/menu.png) | ![Thème sombre](docs/img/theme-sombre.png) |

## Fonctions notables

- **Connecteurs liés** (droits, coudés, courbes) qui suivent les formes ; les coudés **contournent les formes** rencontrées ; poignées de
  **points de passage** (glisser le rond creux au milieu d'un tronçon pour en créer un, Alt+clic pour le retirer, menu « Redresser »).
- **Stylet** : la pression fait varier l'épaisseur du trait (écran, SVG, Excalidraw) ; l'**extrémité gomme** du stylet efface, quel que soit l'outil actif (Windows, via l'API Pointer — voir `wb-pen`). **À valider avec un vrai stylet** : `tableau-blanc --pen-test` affiche un diagnostic et produit un rapport ([docs/STYLET.md](docs/STYLET.md)).
- **Presse-papiers système** : copier/coller d'objets, d'images et de texte.
- **Menu Insérer** : bloc de code (police à chasse fixe, coloration syntaxique générique), **formule LaTeX composée** (vraie typographie TeX, police STIX Two Math embarquée : fractions, racines, sommes, intégrales, matrices ; double-clic pour la modifier, couleur = couleur de trait, source conservée),
  pastilles de vote (outil dédié).
- **Présentation** (F5) : plein écran sans interface, flèches / espace / Page↓ pour changer de page, Échap pour quitter ; **minuteur** (menu Affichage).
- **Pointeur laser** (F) : une traînée rouge qui s'efface, pour montrer sans dessiner ; en présentation, F l'active.
- **Exports** : draw.io, Excalidraw, JSON Canvas, SVG, PNG, **PDF vectoriel**.
- **Agents (MCP)** : `wb-mcp --root <dossier>` expose 16 outils (formes, connecteurs liés, tableaux, code, formules, Mermaid, mise en page, export). **Ressources MCP** (tableau affiché, fichiers) avec notifications d'abonnement, outil `board_changes` (ce que vous avez modifié depuis le dernier regard de l'agent), insertion d'images. **Mode direct** : application ouverte, l'agent agit sur le tableau affiché (une étape d'annulation par appel, il voit votre sélection) ; sinon il modifie le fichier et l'application le recharge seule.
- **Distribution** : icône, archive portable et installation par utilisateur (voir [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)).

## Texte riche et raccourcis

- **Texte riche** : double-clic sur une forme, un tableau ou une flèche, puis sélection de mots (souris ou Maj+flèches) →
  gras (Ctrl+B), italique (Ctrl+I), souligné (Ctrl+U), barré, **couleur** et **surlignage** par mot, via la barre flottante
  ou le panneau de droite. Sans sélection, la mise en forme s'applique à tout le texte ; hors édition, à tout le texte des
  objets sélectionnés. La frappe continue dans le style du caractère précédent.
- **Raccourcis personnalisables** : menu *Affichage → Raccourcis clavier…* (41 commandes, plusieurs raccourcis par commande,
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

## Licence

Double licence **MIT OU Apache-2.0**, à votre choix : voir [LICENSE-MIT](LICENSE-MIT) et [LICENSE-APACHE](LICENSE-APACHE). Les licences des composants tiers embarqués sont dans
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). Contributions : [CONTRIBUTING.md](CONTRIBUTING.md).
