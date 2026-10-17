# Serveur MCP (`wb-mcp`)

Permet à un agent (Claude, ou tout client MCP) de **créer et modifier des tableaux** en appelant des outils. Niveau A : les outils lisent et écrivent
des fichiers `.tableau.json`. Si l'application a ce fichier ouvert, elle le **recharge toute seule** dans la demi-seconde (tant qu'il n'y a pas de
modification locale non enregistrée ; sinon elle prévient et ne remplace rien) : on voit l'agent dessiner en direct.

## Brancher le serveur

```bash
cargo build --release -p wb-mcp
claude mcp add tableau-blanc -- E:\Dev\tableau-blanc\target\release\wb-mcp.exe --root C:\Tableaux
```

Pour un autre client, la configuration est la même : commande `wb-mcp.exe`, arguments `--root <dossier>`, transport stdio.
**Utilisez toujours `--root`** : sans lui l'agent peut lire et écrire n'importe quel fichier accessible à votre compte.
Avec `--root`, tout chemin (relatif ou absolu) doit se trouver sous ce dossier ; `..` et les liens sont neutralisés. Seuls les fichiers `.tableau.json`
sont modifiés ; `export` n'écrit que `.tableau.json`, `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`, `.json`.

## Outils

| Outil | Rôle |
|---|---|
| `create_board` | Crée un tableau vide (refuse d'écraser un fichier existant). |
| `describe_board` | Pages et objets (id, type, texte, position, liens des connecteurs). Lit aussi draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, avec texte, couleurs, taille. |
| `add_connector` | Flèche **liée** entre deux objets (`straight`, `elbow`, `curve`, pointes, étiquette). |
| `add_table` | Tableau de cellules (colonnes ajustées au texte). |
| `add_code_block` | Bloc de code coloré. |
| `add_formula` | Formule LaTeX composée comme en TeX (erreur claire si non gérée). |
| `import_mermaid` | Diagramme Mermaid (flowchart, état, séquence) avec mise en page automatique ; page dédiée possible. |
| `update_object` / `delete_object` | Modifier (texte, position, taille, couleurs) ou supprimer. |
| `auto_layout` | Réorganise un graphe de formes reliées (TB, LR, BT, RL). |
| `add_page` | Nouvelle page. |
| `export` | `.svg`, `.png`, `.pdf`, `.drawio`, `.excalidraw`, `.canvas`, `.tableau.json` d'après l'extension. |

Sans `x`/`y`, un nouvel objet est placé sous le contenu existant. Les ids renvoyés servent à relier ou modifier les objets.
Les erreurs sont renvoyées comme résultat d'outil (`isError`) avec un message en français que l'agent peut exploiter.

## Limites

- Niveau A seulement : pas de mode « direct » avec l'application (le rechargement automatique donne l'illusion, mais l'historique d'annulation de l'application repart de zéro à chaque rechargement).
- Pas de lecture des traits au crayon ni des images au-delà de leur présence ; pas d'insertion d'images.
- Le protocole est implémenté à la main (`initialize`, `ping`, `tools/list`, `tools/call`) ; pas de ressources, d'invites ni de notifications serveur.
- Testé avec un client écrit pour l'occasion (`crates/wb-mcp/tests/session.rs`) **et avec Claude Code 2.1.258** : `claude mcp add` puis `claude mcp list` (✔ Connected), et une demande en langage naturel (Mermaid + formule) a produit un tableau de 6 objets. Pas testé avec Claude Desktop.
- Plusieurs agents écrivant en même temps dans le même fichier : le dernier gagne (chaque outil relit puis réécrit le fichier entier, atomiquement).
