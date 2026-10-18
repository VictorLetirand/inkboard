# Serveur MCP (`inkboard-mcp`)

Permet à un agent (Claude, ou tout client MCP) de **créer et modifier des tableaux** en appelant des outils. Deux modes, choisis automatiquement :

- **Direct** (application ouverte) : l'outil agit sur le tableau **affiché**, dans l'application. Chaque appel est **une étape d'annulation** (Ctrl+Z),
  l'objet créé est sélectionné, `describe_board` indique la **sélection de l'utilisateur** (« modifie ce que j'ai sélectionné »), et rien n'est enregistré
  sur le disque tant que l'utilisateur (ou `save_board`) ne le décide pas. Si le tableau est vide, la vue s'ajuste.
- **Fichier** (application fermée, ou `path` d'un autre fichier) : l'outil lit et écrit le fichier `.inkboard.json` ; si l'application a ce fichier ouvert,
  elle le **recharge toute seule** dans la demi-seconde (sauf modifications locales non enregistrées : elle prévient et ne remplace rien).

En mode direct, `path` est facultatif ; s'il est donné et désigne le fichier affiché, l'outil agit en direct ; s'il désigne un autre fichier, il agit sur ce fichier.
Sans `path` et sans application ouverte, l'outil répond par une erreur claire.

## Brancher le serveur

```bash
cargo build --release -p inkboard-mcp
claude mcp add inkboard -- E:\Dev\inkboard\target\release\inkboard-mcp.exe --root C:\Tableaux
```

Pour un autre client, la configuration est la même : commande `inkboard-mcp.exe`, arguments `--root <dossier>`, transport stdio.
**Utilisez toujours `--root`** : sans lui l'agent peut lire et écrire n'importe quel fichier accessible à votre compte.
Avec `--root`, tout chemin (relatif ou absolu) doit se trouver sous ce dossier ; `..` et les liens sont neutralisés. Seuls les fichiers `.inkboard.json`
sont modifiés ; `export` n'écrit que `.inkboard.json`, `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`, `.json`.

## Comment le mode direct fonctionne

L'application ouvre un serveur local sur `127.0.0.1` (port éphémère, jamais exposé au réseau) et publie son port et un **jeton secret** aléatoire dans
`%APPDATA%\Inkboard\live.json` (supprimé à la fermeture). `inkboard-mcp` lit ce fichier, transmet l'appel avec le jeton, et l'application exécute l'outil sur son fil
d'interface. Une demande sans jeton valide est refusée sans rien exécuter. Les chemins (`path`, `output`) sont résolus et confinés à `--root` par `inkboard-mcp`
avant la transmission. Options : `--no-live` désactive le mode direct ; `--live-file` change le fichier de découverte.

## Outils

| Outil | Rôle |
|---|---|
| `create_board` | Crée un tableau vide (refuse d'écraser un fichier existant). |
| `save_board` | Mode direct : enregistre le tableau affiché (il doit déjà avoir un fichier). |
| `board_changes` | Ce que **l'utilisateur** a modifié depuis votre dernier regard (`describe_board` / `board_changes`) : objets ajoutés, modifiés, supprimés. Vos propres modifications ne sont pas signalées. |
| `add_image` | Image PNG/JPEG/GIF/WebP/SVG (15 Mio au plus) depuis un fichier sous la racine (`source`) ou en base64 ; taille naturelle réduite à 600, ou `w`/`h`. Le type est reconnu d'après le contenu, jamais d'après l'extension. |
| `describe_board` | Pages et objets (id, type, texte, position, liens des connecteurs) ; en mode direct, aussi `selection`. Lit aussi draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, avec texte, couleurs, taille. |
| `add_connector` | Flèche **liée** entre deux objets (`straight`, `elbow`, `curve`, pointes, étiquette). |
| `add_table` | Tableau de cellules (colonnes ajustées au texte). |
| `add_code_block` | Bloc de code coloré. |
| `add_formula` | Formule LaTeX composée comme en TeX (erreur claire si non gérée). |
| `import_mermaid` | Diagramme Mermaid (flowchart, état, séquence) avec mise en page automatique ; page dédiée possible. |
| `update_object` / `delete_object` | Modifier (texte, position, taille, couleurs) ou supprimer. |
| `auto_layout` | Réorganise un graphe de formes reliées (TB, LR, BT, RL). |
| `add_page` | Nouvelle page. |
| `export` | `.svg`, `.png`, `.pdf`, `.drawio`, `.excalidraw`, `.canvas`, `.inkboard.json` d'après l'extension. |

Sans `x`/`y`, un nouvel objet est placé sous le contenu existant. Les ids renvoyés servent à relier ou modifier les objets.
Les erreurs sont renvoyées comme résultat d'outil (`isError`) avec un message en français que l'agent peut exploiter.

## Ressources et notifications

Le serveur déclare les capacités MCP `resources` (avec `subscribe` et `listChanged`) :

- `inkboard://live` : le tableau affiché dans l'application (même contenu que `describe_board`, avec la sélection) — présent seulement quand l'application est ouverte ;
- `file:///…/*.inkboard.json` : les tableaux sous la racine `--root` (3 niveaux de dossiers, 300 fichiers au plus).

`resources/subscribe` sur l'une d'elles : le serveur envoie `notifications/resources/updated` quand le contenu change (fichier modifié, ou — pour le tableau
affiché — modification **ou changement de sélection** par l'utilisateur), et `notifications/resources/list_changed` quand la liste change. Le changement est
détecté par sondage toutes les 600 ms. La première observation après l'abonnement sert de référence (aucune notification pour l'état déjà lu).

**Selon le client**, ces notifications ne sont pas forcément montrées au modèle : un client qui ne gère pas les abonnements les ignore. C'est pourquoi l'outil
`board_changes` existe : l'agent peut demander lui-même ce que l'utilisateur a changé, quel que soit le client.

## Limites

- Plusieurs instances de l'application : la dernière lancée publie le fichier de découverte (les autres ne sont pas pilotables).
- Après un plantage de l'application, un fichier de découverte périmé peut subsister ; `inkboard-mcp` bascule alors en mode fichier.
- En mode direct, l'agent change la page affichée quand il en cible une autre (`page`), et la sélection de l'utilisateur est remplacée par les objets créés.
- Pas de lecture des traits au crayon ni du contenu des images (seulement leur présence, taille et position).
- Le suivi `board_changes` est désactivé au-delà de 20 000 objets (coût linéaire à chaque appel).
- Les notifications de ressources reposent sur un sondage (600 ms) et ne portent pas le détail du changement : l'agent relit la ressource ou appelle `board_changes`.
- Le protocole MCP est implémenté à la main (`initialize`, `ping`, `tools/list`, `tools/call`) ; pas de ressources, d'invites ni de notifications serveur.
- Testé de bout en bout avec un client écrit pour l'occasion (`crates/inkboard-mcp/tests/session.rs`, `crates/inkboard-app/src/tests.rs`) **et avec Claude Code 2.1.258**
  (mode fichier, puis mode direct contre l'application réelle : ajout d'un rectangle, `describe_board` avec `live: true` et sélection). Les ressources, abonnements et notifications sont testés avec le vrai binaire et avec l'application (tests automatisés) mais **pas avec un client MCP réel qui les exploite** (Claude Code a servi pour les outils ; je n'ai pas pu vérifier ce qu'il fait des notifications). Pas testé avec Claude Desktop.
- Plusieurs agents écrivant en même temps dans le même fichier (mode fichier) : le dernier gagne.
