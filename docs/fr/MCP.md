# Serveur MCP (`inkboard-mcp`)
*English version: [MCP.md](../MCP.md)*

Permet à un agent (Claude, ou tout client MCP) de **créer et modifier des tableaux** en appelant des outils. Deux modes, choisis automatiquement :

- **Direct** (application ouverte) : l'outil agit sur le tableau **affiché**, dans l'application. Chaque appel est **une étape d'annulation** (Ctrl+Z),
  l'objet créé est sélectionné, `describe_board` indique la **sélection de l'utilisateur** (« modifie ce que j'ai sélectionné »), et rien n'est enregistré
  sur le disque tant que l'utilisateur (ou `save_board`) ne le décide pas. Si le tableau est vide, la vue s'ajuste.
- **Fichier** (application fermée, ou `path` d'un autre fichier) : l'outil lit et écrit le fichier `.inkboard` ; si l'application a ce fichier ouvert,
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
Avec `--root`, tout chemin (relatif ou absolu) doit se trouver sous ce dossier ; `..` et les liens (y compris un lien servant de fichier) qui mènent hors de ce dossier sont refusés. Seuls les fichiers de tableau
sont modifiés (`.inkboard` ; les anciens noms `.inkboard.json` et `.tableau.json` sont toujours lus et écrits sous leur propre nom, et `create_board` crée des fichiers `.inkboard`) ; `export` n'écrit que `.inkboard`, `.inkboard.json`, `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`, `.json`.

### Installé avec l'application

Les installeurs placent `inkboard-mcp` à côté de l'application (Windows, installation par utilisateur : `%LOCALAPPDATA%\Inkboard\inkboard-mcp.exe`) : rien à compiler, il suffit d'enregistrer ce fichier dans votre client, comme ci-dessous.

### GitHub Copilot (VS Code)

*Pas encore testé avec Inkboard : rédigé d'après la documentation de VS Code.* VS Code lance les serveurs stdio locaux depuis `.vscode/mcp.json` (projet) ou la configuration utilisateur. La clé racine est `servers` (pas `mcpServers`) :

```json
{
  "servers": {
    "inkboard": {
      "type": "stdio",
      "command": "C:\\Users\\VOUS\\AppData\\Local\\Inkboard\\inkboard-mcp.exe",
      "args": ["--root", "D:\\tableaux"]
    }
  }
}
```

- Donnez un chemin absolu pour `command`.
- Passez le chat Copilot en mode **Agent** : les outils MCP ne servent pas en mode *Ask*, et chaque outil demande une approbation.
- VS Code n'utilise que les **outils** du serveur : les ressources et les notifications de changement sont ignorées, l'agent doit donc appeler `board_changes` pour voir ce que vous avez modifié.
- Si le client n'affiche pas le bloc image de `render`, donnez `output` (un `.png` sous le dossier racine) : l'image y est aussi écrite.
- Microsoft 365 Copilot (Teams, Word…) ne peut pas utiliser ce serveur : il n'atteint que des serveurs MCP **distants** en HTTP, et Inkboard ne parle que stdio.

### Claude Desktop et autres clients

*Non testé.* Même commande et mêmes arguments dans la configuration MCP du client (pour Claude Desktop, `mcpServers` dans `claude_desktop_config.json`) : `{"mcpServers": {"inkboard": {"command": "<chemin>\\inkboard-mcp.exe", "args": ["--root", "<dossier>"]}}}`.

### Dans un conteneur (mode fichier)

[`packaging/docker/mcp.Dockerfile`](../../packaging/docker/mcp.Dockerfile) construit le serveur sous forme d'image de conteneur, pour les clients qui démarrent leurs serveurs MCP par Docker et pour les annuaires qui construisent à partir d'un Dockerfile. Dans un conteneur, il n'y a ni fenêtre ni application ouverte : le serveur démarre donc avec `--no-live` et travaille sur les fichiers `.inkboard` du dossier monté (`--root /data`) :

```bash
docker build -f packaging/docker/mcp.Dockerfile -t inkboard-mcp .
docker run -i --rm -v "$PWD/boards:/data" inkboard-mcp
```

Le serveur parle MCP sur l'entrée et la sortie standard (d'où `-i`). L'image tourne sous un utilisateur sans droits (uid 10001) : le dossier monté doit être modifiable par lui. Elle a été vérifiée avec une vraie session MCP (`initialize`, `tools/list` renvoyant les 29 outils, puis `create_board`, `add_shape` et `describe_board` sur un fichier du dossier monté, et un chemin hors de `/data` refusé). Le mode direct (modifier le tableau ouvert à l'écran) demande l'application de bureau et n'existe pas dans un conteneur.

### Pointes de flèche

`add_connector` et `update_object` acceptent `head_start` et `head_end` pour choisir par son nom la pointe de chaque extrémité (`arrow` règle toujours les deux d'un coup ; une pointe nommée l'emporte pour son extrémité). Pointes d'origine : `none`, `arrow`, `triangle`, `dot`, `bar`. Pointes ajoutées, pour les diagrammes UML et entité-relation : `diamond` (composition), `diamond_open` (agrégation), `triangle_open` (héritage, réalisation), `crow_foot` (plusieurs), `crow_foot_one` (un ou plusieurs), `crow_foot_zero` (zéro ou plusieurs), `one_only` (exactement un), `zero_or_one`. `describe_board` avec `detail: "full"` les indique dans `heads`.

Les pointes ajoutées sont stockées dans des champs optionnels séparés du connecteur (`head_start_ext`, `head_end_ext`), à côté de `head_start` / `head_end` d'origine, qui gardent la pointe d'origine la plus proche (un losange est un `dot`, un triangle creux un `triangle`, une patte d'oie une `arrow`, `one_only` une `bar`). Une version qui ne connaît pas ces pointes ignore les nouveaux champs et dessine celle-là : un tableau qui les utilise s'ouvre donc toujours, et un tableau qui ne les utilise pas est écrit exactement comme avant. Dans une session en direct, un invité sur une ancienne version qui modifie un tel connecteur le renvoie sans les pointes ajoutées, qui s'affichent alors avec leur pointe d'origine pour tout le monde. Les fichiers draw.io et Excalidraw utilisent leurs propres symboles (draw.io : `diamond`, `ERmany`, `ERoneToMany`, `ERzeroToMany`, `ERmandOne`, `ERzeroToOne`, avec `endFill=0` pour les creux).

### Côtés d'attache, position de l'étiquette et routes coudées

Trois réglages pour les flèches d'un schéma, dans `add_connector` et `update_object` :

- **`from_side`, `to_side`** (`top`, `bottom`, `left`, `right`, `auto`) : le côté de l'objet auquel une extrémité s'attache. La flèche part (ou arrive) au **milieu de ce côté de la boîte de l'objet** et sort perpendiculairement : une flèche droite part de ce point, une flèche coudée a son premier segment perpendiculaire, une courbe lui est tangente. `auto` (par défaut, et ce qui efface un côté dans `update_object`) garde le comportement automatique. Sans côté, une flèche attachée à une **image** (une icône) utilise désormais aussi le milieu du côté qui fait face à l'autre extrémité, et non un point de la boîte sur la ligne entre les centres (qui pouvait être un coin). Les formes gardent le point de leur contour qui fait face à l'autre extrémité. Reconnecter une extrémité avec `from` / `to` supprime son côté ; donner `from_side` / `to_side` dans le même appel pour en fixer un pour le nouvel objet. `describe_board` avec `detail: "full"` indique `from_side` / `to_side` quand ils sont définis.
- **`label_pos`** (0 à 1, 0,5 par défaut) : position de l'étiquette le long du trajet, 0 au départ et 1 à l'arrivée. **`label_offset`** (unités, de -1000 à 1000, 0 par défaut) : distance au trajet, perpendiculairement, positive à droite du sens de parcours (sous un trajet qui va vers la droite), négative à gauche. Il faut une étiquette (`label` / `text`, dans le même appel ou avant) ; `describe_board` (`detail: "full"`) les indique quand ils sont définis. L'étiquette suit la flèche quand les objets bougent ; `check_layout` (`label_collision`) utilise la même position.
- **Les routes coudées sont séparées.** Les flèches coudées dont le segment du milieu longerait la même ligne en se recouvrant (typiquement une source qui part vers une colonne de cibles, ou deux colonnes reliées par des flèches parallèles) sont réparties sur des canaux parallèles distants de 10 unités, ordonnés pour s'emboîter sans se croiser, et `check_layout` ne les signale plus en `shared_segment`. Limites : seules les flèches sans points de passage sont séparées, parmi les flèches d'une page de 150 flèches coudées et 5000 objets au plus ; des flèches qui partent du même point partagent leur premier segment (un tronc commun, puis elles se séparent) ; une flèche qui doit contourner un obstacle garde sa route.

Une forme peut porter son titre : `align: "left"` avec `valign: "top"` (`top`, `middle`, `bottom` ; `middle` par défaut) dans `add_shape` / `update_object` place le texte en haut à gauche d'un `rect`, si bien qu'un cadre titré est un seul objet (un fond transparent et `send_to_back` laissent le contenu visible). `describe_board` avec `detail: "full"` indique `font.valign` quand il n'est pas `middle`.

Fichiers et formats. Les nouveaux champs ne sont écrits que s'ils sont définis (`side` dans une extrémité, `label_pos` et `label_offset` dans un connecteur, `valign` dans un texte) : un tableau qui ne les utilise pas est écrit exactement comme avant et un ancien fichier s'ouvre tel quel ; une version plus ancienne les ignore. **Le partage en direct passe au protocole v6** : mettre à jour toutes les applications qui hébergent ou rejoignent une session. draw.io : un côté s'écrit `exitX` / `exitY` (départ) et `entryX` / `entryY` (arrivée), et le milieu d'un côté lu dans un fichier draw.io redevient un côté (tout autre point reste une fraction de la boîte) ; la position de l'étiquette est la géométrie `x` de l'arête (-1 à 1 le long de l'arête, soit `label_pos` × 2 − 1) et `y` (décalage) ; `valign` est `verticalAlign`, et un rectangle draw.io dont le texte est en haut ou en bas le garde dans un seul objet. JSON Canvas : `fromSide` / `toSide` dans les deux sens. Excalidraw : `valign` est le `verticalAlign` du texte ; les côtés et la position de l'étiquette n'ont pas d'équivalent et ne sont pas exportés.

## Comment le mode direct fonctionne

L'application ouvre un serveur local sur `127.0.0.1` (port éphémère, jamais exposé au réseau) et publie son port et un **jeton secret** aléatoire dans
`%APPDATA%\Inkboard\live.json` (supprimé à la fermeture). `inkboard-mcp` lit ce fichier, transmet l'appel avec le jeton, et l'application exécute l'outil sur son fil
d'interface. Une demande sans jeton valide est refusée sans rien exécuter. Les chemins (`path`, `output`) sont résolus et confinés à `--root` par `inkboard-mcp`
avant la transmission. Options : `--no-live` désactive le mode direct ; `--live-file` change le fichier de découverte.

## Outils

| Outil | Rôle |
|---|---|
| `create_board` | Crée un tableau vide. Un fichier existant est une erreur qui indique d'utiliser le tableau existant ou de passer `overwrite: true`, qui le remplace par un tableau vide neuf. |
| `save_board` | Mode direct : enregistre le tableau affiché (il doit déjà avoir un fichier). |
| `board_changes` | Ce que **l'utilisateur** a modifié depuis votre dernier regard (`describe_board` / `board_changes`) : objets ajoutés, modifiés, supprimés. Vos propres modifications ne sont pas signalées. |
| `describe_board` | Pages et objets (id, type, texte, position, liens des connecteurs) ; en mode direct, aussi `selection`. `detail: "full"` ajoute les [styles et polices](#lire-les-styles). Lit aussi draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `find_objects` | Trouve des objets par texte, expression régulière, type ou zone, sans lire tout le tableau (lecture seule). |
| `render` | **Image du tableau** (PNG renvoyé directement à l'agent, rien à exporter puis relire). Tout le contenu, une `region`, ou les `ids` à cadrer ; badges d'id facultatifs (`show_ids`) ; `output` écrit aussi le PNG sous la racine. Lecture seule. |
| `check_layout` | **Liste les défauts de mise en page d'une page** avec les ids concernés (chevauchements, texte qui déborde, flèches qui traversent un objet, se croisent ou se superposent, étiquettes en collision, flèches en double ou non attachées, texte trop petit une fois exporté, mise en page déséquilibrée). Lecture seule ; voir [`check_layout`](#check_layout). |
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, avec texte (éventuellement [texte enrichi](#texte-enrichi)), couleurs, taille. `align` (`left`, `center`, `right`) et `valign` (`top`, `middle`, `bottom`) placent le texte : `align: "left"` + `valign: "top"` sur un `rect` est un cadre titré en un seul objet ([détails](#côtés-dattache-position-de-létiquette-et-routes-coudées)). |
| `add_connector` | Flèche **liée** entre deux objets (`straight`, `elbow`, `curve`, pointes, étiquette — éventuellement en texte enrichi ; `from_side` / `to_side` choisissent le côté de chaque objet où la flèche s'attache, `label_pos` / `label_offset` placent l'étiquette, voir [côtés d'attache](#côtés-dattache-position-de-létiquette-et-routes-coudées)). |
| `add_table` | Tableau de cellules (colonnes ajustées au texte) ; les cellules peuvent être en texte enrichi. |
| `add_code_block` | Bloc de code coloré. |
| `add_formula` | Formule LaTeX composée comme en TeX (erreur claire si non gérée). |
| `add_image` | Image PNG/JPEG/GIF/WebP/SVG (15 Mio au plus) depuis un fichier sous la racine (`source`) ou en base64 ; taille naturelle réduite à 600, ou `w`/`h`. Le type est reconnu d'après le contenu, jamais d'après l'extension. |
| `search_icons` | Cherche dans la bibliothèque d'icônes intégrée (environ 28 800 icônes : AWS, Azure, Google Cloud, Cisco, Kubernetes, logos de marques, symboles d'interface…) par mots-clés (`query`, noms en anglais, sans distinction de casse, chaque mot doit correspondre) ; `group`, `set`, `limit` facultatifs (20 par défaut, 100 au plus). Renvoie `{count, truncated, icons: [{id, name, set, group, mono}]}`. Lecture seule. |
| `list_icon_groups` | Les groupes de la bibliothèque d'icônes avec leurs jeux et leur nombre d'icônes (`{total, groups: [{group, sets: [{set, icons}]}]}`), pour savoir quoi chercher. Lecture seule. |
| `add_icon` | Place une icône (`icon` = un `id` issu de `search_icons`) comme objet image auquel `add_connector` peut s'attacher. `size` (plus grand côté, 64 par défaut, de 8 à 2000) ou `w`/`h` (proportions conservées quand un seul est donné) ; `stroke` teinte les icônes monochromes ; `label` ajoute un texte centré sous l'icône, groupé avec elle (`markdown: true` pour du [texte enrichi](#texte-enrichi)). Renvoie `{id, x, y, w, h, icon, label_id?, warnings?}`. |
| `import_mermaid` | Diagramme Mermaid (flowchart, état, séquence, classes, entité-relation) avec mise en page automatique ; page dédiée possible. Un `subgraph` devient un cadre en pointillés avec son titre ; son contenu est disposé à part, donc les cadres ne se chevauchent jamais, et sa `direction` s'applique quand aucun lien ne le relie à l'extérieur. Un lien écrit vers un sous-graphe (`B --> Back`) s'attache à ce cadre. Un `;` dans un libellé entre guillemets (`A["a; b"]`) reste dans le libellé. Une classe ou une entité devient un tableau (nom, attributs, méthodes ; ou type, nom, clés) ; héritage et réalisation se terminent par un triangle creux, composition et agrégation par un losange, et un lien d'entités se termine par des symboles de patte d'oie et écrit ses cardinalités. Non pris en charge : pie, mindmap, gantt (une erreur explicite le dit). |
| `update_object` | Modifier le texte ([texte enrichi](#texte-enrichi) facultatif), la position, la taille, les couleurs, le [style de trait, la police, le verrou, la rotation](#update_object), le tracé / les pointes / les extrémités d'un connecteur, ou une cellule de tableau. |
| `delete_object` | Supprime un objet (`id`) ou plusieurs (`ids`). |
| `move_objects` | Déplace des objets ensemble (en relatif ou vers une position) ; les connecteurs liés suivent. |
| `arrange_objects` | Aligne, répartit, dispose en grille (`grid`), empile (premier plan / arrière-plan), groupe, dégroupe, verrouille, déverrouille. |
| `add_frame` | **Cadre de regroupement** autour d'objets existants en un appel : un seul `rect` ajusté à l'union des `ids` plus `padding`, avec son propre titre en haut à gauche, placé derrière eux ; `frame` le réajuste plus tard. Voir [`add_frame`](#add_frame). |
| `duplicate_objects` | Copie des objets (une ou plusieurs copies, avec un décalage) ; les connecteurs entre objets copiés sont conservés. |
| `format_text` | Gras, italique, souligné, barré, couleur, surlignage sur du texte **déjà présent sur le tableau** : une partie du texte d'une forme, l'étiquette d'un connecteur ou une cellule de tableau. |
| `auto_layout` | Réorganise un graphe de formes, images et tableaux reliés (TB, LR, BT, RL) ; `gap_x` / `gap_y`, `ids` ; une icône et son libellé forment un seul nœud et un cadre est disposé autour de son contenu. Voir [`auto_layout`](#auto_layout). |
| `add_page` | Nouvelle page. |
| `manage_pages` | Liste, renomme ou supprime des pages (`add_page` les crée). |
| `batch` | **Plusieurs appels d'outils en une seule demande** (1 à 200), exécutés dans l'ordre sur un même tableau, avec des références aux résultats précédents (`"$0.id"`), tout ou rien par défaut, une seule étape d'annulation. |
| `export` | `.svg`, `.png`, `.pdf`, `.drawio`, `.excalidraw`, `.canvas`, `.inkboard` d'après l'extension. Les dossiers manquants de `output` sont créés (sous la racine ; `render` fait de même pour son `output`). Pour un `.png`, `width` (16 à 8000) donne la largeur en pixels quelle que soit la taille du diagramme, pour que plusieurs diagrammes aient la même largeur ; ou `scale` (0,1 à 8, 2 par défaut) donne les pixels par unité du tableau. Pas les deux. |

Sans `x`/`y`, un nouvel objet est placé sous le contenu existant. Les ids renvoyés servent à relier ou modifier les objets.
Les erreurs sont renvoyées comme résultat d'outil (`isError`) avec un message en anglais que l'agent peut exploiter.

## Icônes

La bibliothèque d'icônes de l'application (la palette) est aussi à la disposition de l'agent, hors ligne : le pack est intégré à `inkboard-mcp`, ce qui alourdit
le serveur autonome d'environ 8 Mo. Usage typique : `list_icon_groups` une fois pour voir les groupes, `search_icons` (`{"query": "lambda", "group": "AWS"}`),
puis `add_icon` avec l'`id` renvoyé, puis `add_connector` entre les ids d'images renvoyés.

- **Id d'icône** : `<id du jeu>/<nom de l'icône>`, par exemple `drawio-aws4/Lambda` ou `simple-icons/GitHub`. Quelques jeux contiennent deux icônes de même nom ; la
  seconde reçoit le suffixe `#2` (`simple-icons/Backstage#2`). Les ids viennent du pack : copiez-les depuis `search_icons`, ne les construisez pas.
- **`group` / `set`** (recherche) : `group` est un nom de groupe tel que listé par `list_icon_groups` (sans distinction de casse) ; `set` est le titre exact ou l'id d'un jeu.
  Une valeur inconnue est une erreur qui liste les groupes valides, ou les jeux les plus proches.
- **Couleurs** : les icônes avec `mono: true` (icônes générales, formes de diagrammes, matériel de baie…) sont d'une seule couleur et prennent la couleur `stroke` de l'objet ;
  sans `stroke`, la couleur de texte du thème (elles restent donc lisibles sur un tableau sombre). Les icônes en couleurs (clouds, marques) gardent leurs propres couleurs : un `stroke` donné
  pour elles est ignoré, et le résultat le signale dans `warnings`.
- **Placement** : comme la palette, l'image n'a ni contour ni remplissage ; la même icône placée plusieurs fois n'est stockée qu'une fois dans le fichier. Sans `x`/`y`,
  l'icône est placée sous le contenu existant. Avec `label`, la zone de texte est centrée sous l'icône (6 unités plus bas) et groupée avec elle, de sorte que le couple se déplace
  ensemble ; l'`id` renvoyé est celui de l'image, qui est l'objet à relier.
- **Erreurs** (le tableau reste inchangé) : `query` vide ou trop longue (200 caractères au plus), `limit` hors de 1–100, `group` / `set` inconnu, `size` /
  `w` / `h` hors de 8–2000, `size` donné avec `w` / `h`, `stroke` invalide, `label` de plus de 500 caractères, et icône inconnue : le message donne jusqu'à 5
  ids proches (`icon not found: "ec2" ; did you mean: … ?`).
- La recherche porte sur le nom de l'icône, le titre de son jeu et son groupe, sans distinction de casse (mais pas des accents : les noms sont en anglais).
- En mode fichier, les deux outils en lecture seule ont tout de même besoin du `path` d'un tableau (il est chargé, jamais réécrit) ; en mode direct ils n'ont besoin de rien.

## Voir le tableau : `render`

`render` renvoie un PNG de la page sous forme de bloc de contenu MCP **image**, suivi d'un bloc texte avec une description JSON de l'image. Il ne modifie jamais le tableau, le fichier ni l'historique d'annulation, en mode direct comme en mode fichier.

| Paramètre | Signification |
|---|---|
| `path`, `page` | Comme pour les autres outils (`path` facultatif en mode direct ; `page` vaut la première par défaut). |
| `region` | `{x, y, w, h}` en unités du tableau (les mêmes que `describe_board`) : montre exactement ce rectangle. `w` et `h` doivent être supérieurs à 0. |
| `ids` | Ids d'objets à cadrer : l'image montre leur boîte englobante plus `padding`, et dessine toujours ce qui les entoure. Les ids inconnus sont listés dans l'erreur. Non combinable avec `region` (erreur « give either region or ids »). |
| `padding` | Marge autour du contenu ou des ids cadrés, en unités du tableau (24 par défaut). Non ajoutée à une `region` explicite. |
| `max_px` | Plus grand côté de l'image en pixels (1400 par défaut, de 16 à 3000). Une petite région est agrandie 4 fois au plus. |
| `show_ids` | `true` dessine un petit badge blanc avec l'id de chaque objet visible par-dessus lui (coin supérieur gauche ; milieu d'une flèche ; les traits au crayon n'en ont pas), afin de relier ce que l'agent voit aux ids qu'attendent `update_object` ou `add_connector`. Seule l'image est touchée, pas le tableau. |
| `output` | Écrit aussi le PNG dans ce fichier, pour les clients qui n'affichent pas les images. Doit se terminer par `.png` et se trouver sous `--root` (comme tout chemin) ; un fichier de tableau ne peut jamais être écrasé ainsi. |

Le bloc texte contient `width_px`, `height_px`, `region` (le rectangle effectivement montré), `scale` (pixels par unité du tableau), `ids_shown` (badges dessinés ; 0 sans `show_ids`) et `output` quand il est donné. Une image dont le PNG dépasserait 4 Mio est automatiquement produite plus petite ; le bloc contient alors aussi `max_px_used` et une `note`. Une page vide est une erreur (`the page is empty: nothing to render`). Le texte est mesuré approximativement (comme dans `export`), si bien que l'image peut différer un peu de ce que montre l'application.

### Résultats d'outil avec une image (convention, pour les développeurs)

Un résultat d'outil est une valeur JSON, transportée telle quelle par le protocole direct (une ligne JSON par réponse). La clé réservée de premier niveau `_image` (`{"mime": "image/png", "data_base64": "…"}`) marque un résultat qui comprend une image : `inkboard-mcp` (`tools/call`) répond alors avec `content = [{"type": "image", "data": …, "mimeType": …}, {"type": "text", "text": <le JSON sans _image>}]`. Tout autre résultat reste un unique bloc texte, exactement comme avant. Comme l'image voyage dans la ligne JSON, une réponse directe peut faire plusieurs Mio (le PNG est limité à 4 Mio, soit 5,5 Mio en base64).

## Arguments inconnus

Un appel (y compris une opération de `batch`) qui contient un argument absent des propriétés du schéma d'entrée de l'outil s'exécute quand même, mais le résultat porte une entrée `warnings` du type `unknown argument "bold" ignored (did you mean "title_bold"?)` ; la propriété la plus proche est suggérée si sa distance d'édition est d'au plus 2 ou si elle partage un préfixe. `path` et `page` sont toujours acceptés. Les `warnings` propres à l'outil sont conservés.

## `check_layout`

Liste les défauts mesurables d'une page, avec les ids des objets concernés, pour les corriger sans juger une image. Lecture seule. Le texte est mesuré approximativement et les objets sont comparés par leur rectangle englobant : un signalement est un indice fort, pas une preuve (vérifiez avec `render` en cas de doute).

Paramètres : `path`, `page` ; `ids` (ne garde que les problèmes qui touchent ces objets) ; `export_width` (largeur d'export prévue en pixels, 100 à 20000, 1600 par défaut). Résultat : `{"ok", "issues": [{kind, severity, ids, message, ...}], "metrics": {...}}`.

- **Erreurs** : `overlap` (deux boîtes qui se recouvrent de plus de 2 unités, hors contenant/contenu et hors même groupe), `text_overflow` (texte plus haut que sa zone), `word_split` (mot plus large que la zone de texte), `connector_through_object` (le trajet d'une flèche traverse un objet qui n'est ni sa source, ni sa cible, ni un cadre contenant l'une d'elles), `unbound_connector` (extrémité non attachée), `duplicate_connector` (plusieurs flèches entre les deux mêmes objets).
- **Avertissements** : `text_outside_frame` (texte à cheval sur la bordure d'un cadre), `connector_crossing` (deux flèches se croisent, hors extrémité commune), `shared_segment` (deux flèches sur la même ligne sur plus de 20 unités, sauf même cible ou premier segment commun de flèches parties du même point ; les flèches coudées qui se recouvriraient sont d'abord séparées par le routage), `label_collision` (étiquette de flèche, à sa position `label_pos`, sur un objet, une autre étiquette ou une bordure de cadre), `small_text` (texte sous 10 px une fois la page exportée à `export_width` : taille x `export_width` / largeur du contenu), `unbalanced` (rapport largeur/hauteur hors de 0,6 à 2,2, ou au moins 3 cellules vides sur une grille 3 x 3 du contenu quand il y a 6 objets ou plus ; `staircase` : deux cadres de premier niveau côte à côte dont les hauts diffèrent de plus de 60 unités, avec leurs ids ; `coverage` : à partir de 3 objets de premier niveau, leurs boîtes couvrent moins de 35 % de leur boîte englobante), `sparse_frame` (cadre dont le contenu couvre moins de 30 % de sa surface, ou qui laisse une bande vide de plus de 200 unités d'un côté ; `ids` : `[cadre]`, le message propose de rappeler `add_frame` avec `frame=<id>`). Métriques supplémentaires : `coverage` et `frame_top_offset`.
- `metrics` : `objects`, `connectors`, `content_box`, `aspect`, `min_text_px`, `empty_cells`, `export_width`.

## Lire les styles

`describe_board` accepte `detail` : `"brief"` (par défaut, la sortie est inchangée) ou `"full"`. Avec `full`, chaque objet a aussi :

| Champ | Contenu |
|---|---|
| `style` | `{stroke, fill, width, dash, opacity, roundness}` : les couleurs sont `"#rrggbb"`, `"ink"` ou (`fill`) `null` ; `dash` vaut `solid`, `dashed` ou `dotted`. |
| `font` | `{family, size, align}` pour les formes et les étiquettes de connecteurs (`sans`, `hand`, `serif`, `mono` ; `left`, `center`, `right`) ; `{family, size}` pour les tableaux. |
| `z` | Indice d'empilement dans la page : plus il est grand, plus l'objet est proche du premier plan. |
| `group` | Id du groupe, quand l'objet est groupé. |
| `locked` | `true`, seulement quand l'objet est verrouillé. |
| `rotation` | Degrés, seulement pour une forme ou une image pivotée. |
| connecteurs | `route` (`straight`, `elbow`, `curve`), `arrow` (`end`, `start`, `both`, `none`), `stroke` ; `heads` seulement pour les pointes autres que la flèche simple (`triangle`, `dot`, `bar` et les [pointes ajoutées](#pointes-de-flèche)). |
| tableaux | `cells` : le texte brut de chaque cellule, ligne par ligne. |

`selection` est inchangé.

## `find_objects`

Lecture seule. Examine une page (`page`) ; tous les filtres donnés doivent correspondre.

| Paramètre | Signification |
|---|---|
| `text` | Sous-chaîne, sans distinction de casse, du texte d'une forme, de l'étiquette d'un connecteur, des cellules d'un tableau ou du LaTeX d'une formule. |
| `regex` | Expression régulière (syntaxe Rust, 500 caractères au plus, recherchée n'importe où ; `(?i)` pour ignorer la casse). Alternative à `text`. |
| `type` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, `connector`, `table`, `image`, `formula`, `pen`. |
| `in_region` | `{x, y, w, h}` : objets dont la boîte englobante **chevauche** le rectangle. |
| `limit` | 1 à 500, 50 par défaut. |

Résultat : `{"count": n, "truncated": bool, "objects": [...]}` où `count` est le nombre de **toutes** les correspondances, `truncated` indique qu'il y en a plus que `limit`, et `objects` reprend les mêmes résumés que `describe_board` (brief).
Un `text`/`regex` vide ou invalide, les deux donnés, un `type` inconnu, un `in_region` ou un `limit` incorrect sont des erreurs. Le moteur d'expressions régulières s'exécute en temps linéaire et sa taille est limitée.

## `update_object`

Les paramètres existants (`text`, `markdown`, `x`, `y`, `w`, `h`, `fill`, `stroke`) sont inchangés. Nouveaux paramètres :

| Paramètre | Valeurs | S'applique à |
|---|---|---|
| `stroke_width` | 0 à 100 (0 : pas de contour) | tout objet |
| `dash` | `solid`, `dashed`, `dotted` | tout objet |
| `opacity`, `roundness` | 0 à 1 | tout objet |
| `font_size` | 4 à 400 | formes, étiquettes de connecteurs, tableaux |
| `font_family` | `sans`, `hand`, `serif`, `mono` | formes, étiquettes de connecteurs, tableaux |
| `align` | `left`, `center`, `right` | formes, étiquettes de connecteurs |
| `valign` | `top`, `middle`, `bottom` : place verticale du texte dans la forme (`align: left` + `valign: top` : un titre en haut à gauche d'un `rect`) ; aussi dans `add_shape` | formes |
| `locked` | `true` / `false` | tout objet |
| `rotation` | -360 à 360, angle **absolu** en degrés (0 remet à zéro), autour du centre | formes, images |
| `route` | `straight`, `elbow`, `curve` | connecteurs |
| `arrow` | `end`, `start`, `both`, `none` | connecteurs |
| `head_start`, `head_end` | nom d'une [pointe](#pointes-de-flèche) ; remplace ce que `arrow` a donné à cette extrémité | connecteurs |
| `from`, `to` | id d'une forme, d'un tableau ou d'une image : reconnecte cette extrémité (mêmes règles que `add_connector`) | connecteurs |
| `from_side`, `to_side` | `top`, `bottom`, `left`, `right` : le milieu de ce côté de l'objet, la flèche sortant perpendiculairement ; `auto` l'efface ; aussi dans `add_connector`, voir [côtés d'attache](#côtés-dattache-position-de-létiquette-et-routes-coudées) | connecteurs |
| `label_pos`, `label_offset` | `label_pos` de 0 à 1 (0,5 : le milieu) : place de l'étiquette le long du trajet ; `label_offset` de -1000 à 1000 unités : distance à côté du trajet (positive : à droite du sens de parcours) ; aussi dans `add_connector` | connecteurs avec étiquette |
| `head_scale` | 0,5 à 4, taille des pointes (1 : par défaut ; les crans S / M / L / XL du panneau valent 0,6 / 1 / 1,6 / 2,4) ; aussi dans `add_connector` | connecteurs |
| `shadow` | `true` / `false` : ombre portée ; aussi dans `add_shape` | formes, images, tableaux, connecteurs |
| `recolor` | remplace des couleurs d'une image SVG : `{"#e34f26": "#0055ff"}` ou `[["#e34f26", "#0055ff"]]` ; remplace la liste précédente, `{}` rend les couleurs d'origine (256 paires au plus). `describe_board` avec `detail: "full"` liste les couleurs trouvées dans le fichier | images SVG (pas les icônes monochromes ni les formules) |
| `cell` + `text` | `[ligne, colonne]` à partir de 0 : le nouveau texte de cette cellule de tableau (`markdown` fonctionne comme partout) ; la colonne s'élargit si nécessaire | tableaux |

- **Tout ou rien** : chaque paramètre est d'abord validé ; une valeur inconnue ou hors limites est une erreur qui nomme le paramètre et les valeurs permises, et rien n'est modifié.
- Un paramètre qui ne convient pas à l'objet (`rotation` sur un trait au crayon, `route` sur un rectangle…) est **ignoré** et listé dans `warnings` (avec les problèmes de Markdown, voir [Texte enrichi](#texte-enrichi)). La même liste des paramètres ignorés est encore renvoyée sous `avertissements`, **obsolète** (conservé pour les clients existants ; utilisez `warnings`). `cell` sur autre chose qu'un tableau est une erreur.
- Les paramètres de police sur un connecteur s'appliquent à son étiquette ; sans étiquette ils sont signalés (donnez aussi `text`).
- **Les objets verrouillés ne sont pas protégés contre les outils** : `update_object`, `move_objects`, `duplicate_objects` (la copie reste verrouillée) et `delete_object` agissent sur eux. Seuls l'alignement et la répartition de `arrange_objects` les ignorent (ainsi que les objets d'un calque verrouillé).

```json
{"name": "update_object", "arguments": {"id": "…", "dash": "dashed", "opacity": 0.6, "font_family": "mono", "rotation": 15}}
{"name": "update_object", "arguments": {"id": "…", "cell": [1, 2], "text": "**42**", "markdown": true}}
```

## `move_objects`

`ids` (1 à 2000) et **soit** `dx`, `dy` (relatif ; l'un des deux suffit) **soit** `x`, `y` (nouveau coin supérieur gauche de la **boîte englobante de tout le lot** ; pour un seul objet, c'est le `x`/`y` de `update_object`). Donner les deux formes, ou aucune, est une erreur.
Les connecteurs liés aux objets déplacés suivent ; un connecteur déplacé seul ne déplace que ses extrémités libres. Résultat : `{"moved": n, "dx": …, "dy": …}`.

## `arrange_objects`

`ids` (1 à 2000) et `action` :

| `action` | Effet |
|---|---|
| `align_left`, `align_center_x`, `align_right`, `align_top`, `align_center_y`, `align_bottom` | Aligne sur la boîte englobante des objets. Au moins 2 objets non verrouillés. |
| `distribute_horizontal`, `distribute_vertical` | Écarts égaux entre les deux objets les plus extérieurs. Au moins 3 objets non verrouillés. |
| `bring_to_front`, `bring_forward`, `send_backward`, `send_to_back` | Ordre d'empilement (`z`). |
| `group` | Au moins 2 objets partagent un nouveau groupe ; le résultat contient `group`. |
| `ungroup` | Libère les objets listés (résultat : `ungrouped`). |
| `lock`, `unlock` | Verrouille / déverrouille les objets. |
| `grid` | Dispose les objets en grille : `columns` (obligatoire, ≥ 1), ligne par ligne dans l'ordre de `ids`. `gap_x`, `gap_y` (80 par défaut) ; `origin` `{x, y}` = coin haut gauche de la grille (par défaut : coin haut gauche de la boîte actuelle des objets) ; `cell` `"max"` (défaut : chaque cellule aussi grande que le plus grand objet) ou `"fit"` (chaque colonne aussi large et chaque ligne aussi haute que son plus grand membre) ; `align` `"center"` (défaut) ou `"start"` dans la cellule. Une icône et son libellé (un groupe) comptent pour **un seul** élément, placé selon la boîte du groupe. Les connecteurs liés suivent ; les connecteurs de `ids` et les objets verrouillés sont ignorés (`skipped_connectors`, `skipped_locked`). Une seule étape d'annulation. Résultat : `columns`, `rows`, `placed` (`id`, `x`, `y` de chaque élément). |

Les groupes ne sont **pas développés** : listez tous les membres pour déplacer, aligner ou dégrouper un groupe entier.
Les objets verrouillés (et ceux d'un calque verrouillé) sont ignorés par `align_*` et `distribute_*` et listés dans `skipped_locked`. Le moteur seul ne fait rien quand il y a trop peu d'objets non verrouillés (moins de 2 pour aligner, 3 pour répartir) ; l'outil renvoie une erreur à la place.

## `add_frame`

Trace le cadre qu'un agent dimensionnerait sinon à la main. `ids` (les objets à entourer ; les autres membres de leurs groupes, comme le libellé d'une icône, sont inclus) donne **un seul `rect`** dont le texte propre est le titre (`align: left`, `valign: top`, gras) et dont la boîte est l'union des boîtes des `ids` (mêmes bornes que les vérifications de mise en page) plus `padding` (40 par défaut, pas négatif) ; avec un titre, le haut reçoit la hauteur du titre (`title_font_size` × 1,3 pour une ligne) + 12 de plus. Un titre plus large que le cadre l'élargit (`warnings` le dit). Défauts : `fill` `#f1f3f5`, `stroke` `#adb5bd`, `dash` `solid`, `title_font_size` 18, `title_bold` vrai, `roundness` 0,15 (comme un `rect` d'`add_shape`). Renvoie `{id, x, y, w, h}`.

Le cadre est placé **juste sous le plus bas des objets entourés** : un cadre autour d'objets déjà dans un autre cadre est donc dessiné au-dessus du cadre extérieur et sous son propre contenu ; entourer un cadre place le nouveau sous lui. Il n'est pas groupé avec son contenu : après avoir déplacé le contenu, rappelez `add_frame` avec `frame` (son id) et `ids` pour **réajuster** le même cadre (son titre et sa couleur sont conservés ; `title`, `title_color`, `fill`, `stroke`, `dash`, `roundness` ne changent que s'ils sont donnés). `x`, `y`, `w`, `h` (les quatre) donnent un rectangle exact à la place de l'union ; `ids` est alors facultatif et ne sert qu'à l'empilement (sans lui, le cadre va à l'arrière-plan). Le titre est en gris foncé `#343a40` par défaut (`title_color`, `"#rrggbb"` ou `"ink"`, indépendant de la couleur de bordure) et se place à 16 unités des bordures gauche et haute pour dégager le coin arrondi (le haut gagne donc hauteur du titre + 20). Des ids inconnus et un `padding` négatif sont des erreurs.

## `auto_layout`

Dispose les formes, images et tableaux reliés par des connecteurs en graphe en couches (`direction` `TB` par défaut, `LR`, `BT`, `RL`) ; le coin haut gauche de ce qui est disposé reste en place, et les connecteurs coudés sans points de passage deviennent des lignes droites avec points de passage autour des couches. Sans option, l'écart est de 84 entre couches et 48 entre nœuds d'une couche.

- `gap_x`, `gap_y` (0 à 10000) : écarts horizontal et vertical. Pour `TB` / `BT`, `gap_y` sépare les couches et `gap_x` les nœuds d'une couche ; pour `LR` / `RL`, c'est l'inverse.
- `ids` : ne dispose que ces objets et les connecteurs **entre** eux ; tout le reste n'est pas touché.
- **Icônes :** une icône et son libellé (un groupe) bougent comme un seul nœud dont la boîte est l'union du groupe. Idem pour tout groupe (un groupe dont un membre est verrouillé ne bouge pas).
- **Cadres :** un `rect` qui contient d'autres nœuds disposés est un cadre et il est disposé comme un cluster, à la façon d'un `subgraph` Mermaid de `import_mermaid` : son contenu est disposé à part, puis le cadre est réajusté autour (marge 40, plus la place de son titre) et participe comme un seul bloc à la disposition de ce qui l'entoure. Les cadres imbriqués fonctionnent ; un connecteur lié à un cadre relie le bloc. Un cadre est reconnu par sa géométrie (sa boîte contient les boîtes d'autres nœuds de `ids`), qu'il vienne ou non d'`add_frame`.

Résultat : `{layout, objects, nodes, frames}` (`nodes` compte un groupe une fois).

## `manage_pages`

| `action` | Signification |
|---|---|
| `list` | `{"pages": [{"number", "name", "objects"}]}`. |
| `rename` | `page` (numéro à partir de 1 ou nom) et `name` (non vide, 120 caractères au plus, espaces de bord retirés). `warnings` signale si une autre page porte le même nom. |
| `delete` | `page` ; supprime aussi ses objets (`objects_removed`). La dernière page d'un tableau ne peut pas être supprimée. |

`rename` et `delete` exigent `page` (la première page n'est jamais supposée). `add_page` crée des pages.

## `delete_object`

`id` (un objet, résultat `{"deleted": "<id>"}`) **ou** `ids` (1 à 2000, résultat `{"deleted": ["<id>", …]}`) ; les deux ensemble, ou aucun, est une erreur. Si l'un des `ids` n'existe pas, rien n'est supprimé. Les connecteurs liés aux objets supprimés restent, détachés.

## `duplicate_objects`

`ids`, `dx`, `dy` (40, 40 par défaut), `count` (1 à 50, 1 par défaut ; 10 000 nouveaux objets au plus au total). La copie *k* est décalée de *k* × (`dx`, `dy`) par rapport aux originaux.
Les connecteurs **entre** objets copiés sont copiés et reliés aux copies ; un connecteur relié à un objet **non** copié devient libre à cette extrémité dans la copie. Les groupes sont copiés comme de nouveaux groupes. Les copies sont ajoutées au-dessus.
Résultat : `{"created": n, "sources": [...], "ids": [...]}` — `sources` sont les ids copiés dans l'ordre de la page ; `ids` sont les nouveaux : copie 1 dans l'ordre de `sources`, puis copie 2…

```json
{"name": "duplicate_objects", "arguments": {"ids": ["…", "…"], "dx": 0, "dy": 120, "count": 3}}
```

## Texte enrichi

Par défaut, `text` est brut : `**x**` reste `**x**`. Passez **`markdown: true`** à `add_shape` (`text`), `update_object` (`text` d'une forme ou d'un connecteur),
`add_table` (chaque cellule) ou `add_connector` (`label`) pour écrire du texte mis en forme.

| Écrire | Résultat |
|---|---|
| `**gras**` | gras |
| `*italique*` ou `_italique_` | italique |
| `__souligné__` | souligné |
| `~~barré~~` | barré |
| `{color=#e03131}texte{/}` | couleur du texte (`#rrggbb`, `#rgb`, ou `ink`, la couleur de texte du thème) |
| `{bg=#ffec99}texte{/}` | surlignage derrière le texte |
| `{color=#e03131 bg=ink}texte{/}` | les deux dans une seule balise |
| `\*` `\_` `\~` `\{` `\}` `\\` | le caractère lui-même |

- Tout se combine et s'imbrique : `{color=#e03131}**attention :** *vérifier* la {bg=#ffec99}saisie{/}{/}`. `{/}` ferme la dernière balise ouverte ; les autres marqueurs se ferment à leur seconde occurrence. Les couches peuvent se chevaucher (`**a *b** c*`).
- Les sauts de ligne (`\n`) sont inchangés.
- Échappez un `*`, `_`, `~`, `{`, `}` ou `\` littéral avec une barre oblique inverse, par exemple dans un produit de nombres (`2 \* 3`) ou un identifiant (`__init__` est souligné : écrivez `\_\_init\_\_`). Un `_` isolé dans un mot (`snake_case_name`) reste brut ; un `~` seul et un `*` ou `_` qui ne ferme rien sont conservés tels quels.
- **Un balisage invalide n'est jamais une erreur et ne fait jamais perdre de texte** : un marqueur non fermé, une paire vide (`****`), un `{/}` qui ne ferme rien, une couleur invalide (`{color=red}` : seuls `#rrggbb`, `#rgb` et `ink` sont admis) ou un attribut inconnu restent dans le texte tels qu'écrits, et le résultat les liste dans un tableau `warnings` de messages (le champ est absent quand tout a été compris). Dans un tableau, chaque message commence par `cell [row, column]:` (à partir de 0).
- La forme est ajustée après la modification comme avant ; le gras compte dans la largeur du texte.

```json
{"name": "add_shape", "arguments": {"geom": "sticky", "markdown": true,
  "text": "**Deploy**\n{color=#e03131}blocked{/} until {bg=#ffec99}review{/}"}}
```

## `format_text`

Change la mise en forme d'un texte déjà présent sur le tableau, comme si on le sélectionnait dans l'application.

| Paramètre | Signification |
|---|---|
| `id` (obligatoire) | Une forme, un connecteur avec étiquette, ou un tableau (avec `cell`). |
| `match` | Sous-chaîne à mettre en forme (sensible à la casse) : la première occurrence, ou chacune avec `all: true`. |
| `range` | `[début, fin)` : indices de caractères du texte brut, `début` inclus, `fin` exclu (caractères, pas octets). |
| `cell` | `[ligne, colonne]`, à partir de 0 : obligatoire pour un tableau, invalide sinon. |
| `bold`, `italic`, `underline`, `strike` | `true` applique, `false` retire (jamais une bascule). |
| `color`, `highlight` | `"#rrggbb"` / `"ink"` définit, `null` retire. |
| `clear` | `true` retire d'abord toute la mise en forme de la partie ; les autres options s'appliquent ensuite (`clear` + `bold` = réinitialiser, puis gras). |

Sans `match` ni `range`, tout le texte (de la cellule) est mis en forme. `match` avec `range`, `all` sans `match`, une sélection vide ou hors limites, une couleur invalide, un appel qui ne demande rien,
un `match` introuvable (`text not found: "…" (the text is "…")`, avec le début du vrai texte) et un objet sans texte sont des erreurs ; le tableau reste intact.

Résultat : `{"id": "…", "changed": n}` — `n` est le nombre de caractères dont la mise en forme a réellement changé (`0` : déjà mis en forme ainsi).

```json
{"name": "format_text", "arguments": {"id": "…", "match": "blocked", "all": true, "bold": true, "color": "#e03131"}}
{"name": "format_text", "arguments": {"id": "…", "cell": [0, 1], "highlight": "#ffec99"}}
```

## Lire le texte mis en forme

`describe_board` (et `board_changes`) gardent leur sortie **telle quelle** pour le texte brut. Seulement quand un texte porte une mise en forme, l'objet a aussi :

- `markdown` (formes, et connecteurs pour leur étiquette) : le texte dans la syntaxe ci-dessus, prêt à être redonné à un appel avec `markdown: true` ; `text` / `label` contiennent toujours le texte brut ;
- `cells_markdown` (tableaux) : la grille lignes × colonnes de chaque cellule dans la même syntaxe, seulement si au moins une cellule est mise en forme.

```json
{"id": "…", "type": "rect", "text": "Deploy blocked", "markdown": "**Deploy** {color=#e03131}blocked{/}", "x": 0, "y": 0, "w": 180, "h": 90}
```

Limites : mettre en forme une cellule de tableau se fait avec `add_table` (`markdown: true`) ou `format_text` (`cell`) ; `update_object` ne change toujours pas une cellule. Les polices et les tailles ne font pas partie du texte enrichi.

## Beaucoup d'opérations en un appel : `batch`

`batch` exécute une liste d'opérations, chacune de la forme `{"tool": "<nom>", "args": {…}}`, avec les arguments que l'outil prend normalement. C'est le moyen rapide de construire ou de retravailler un tableau : pas d'aller-retour par objet, et les ids renvoyés par une opération peuvent servir aux suivantes.

```json
{"path": "plan.inkboard", "operations": [
  {"tool": "add_shape", "args": {"geom": "rect", "text": "Client"}},
  {"tool": "add_shape", "args": {"geom": "rect", "text": "Server"}},
  {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id", "label": "request"}}
]}
```

| Paramètre | Signification |
|---|---|
| `operations` | **Obligatoire.** 1 à 200 opérations, exécutées dans l'ordre. |
| `atomic` | `true` par défaut : tout ou rien (voir ci-dessous). `false` : conserve ce qui a réussi avant la première erreur. |
| `path`, `page` | Comme pour les autres outils. Ils s'appliquent à **chaque** opération ; une opération peut nommer une autre `page` dans ses propres `args`. Une opération sans `page` agit toujours sur la `page` du lot (par défaut : la première page), quoi qu'aient fait les opérations précédentes (même `add_page`). |

Tout outil du serveur peut être utilisé dans un lot, y compris ceux ajoutés par des versions ultérieures. **Interdits** : `batch` (pas d'imbrication), `create_board`, `save_board`, `board_changes`, `export` et `render` (ils écrivent ailleurs que sur le tableau ou fausseraient le suivi des changements). L'erreur est `tool X cannot be used inside a batch`.

### Références

Une chaîne qui est **exactement** `"$N.chemin"` est remplacée, avant l'exécution de l'opération, par une valeur du **résultat** de l'opération `N` (comptée à partir de 0, et `N` doit être une opération antérieure). Le chemin est fait de clés et d'indices de liste séparés par des points : `"$0.id"`, `"$3.ids.1"` (deuxième id créé par un `import_mermaid` à l'indice 3). La valeur garde son type (chaîne, nombre, liste…), et les références sont trouvées n'importe où dans `args`, y compris dans les listes et les objets.

- Seule la **chaîne entière** est une référence : un `text` comme `"see $0.id"` reste littéral, de même qu'une chaîne qui ne ressemble pas à `$N.clé` (`"$5"`, `"cost $0.id."`). Attention à un texte exactement semblable à une référence, comme `"$3.99"` : il serait pris pour l'une d'elles (écrivez `"US$3.99"` ou `"$ 3.99"`).
- Une référence à une opération qui n'a pas encore été exécutée, ou à un chemin que le résultat n'a pas, est une erreur qui nomme l'opération et la référence.

### Erreurs et atomicité

- **`atomic: true`** (par défaut). À la première erreur, toutes les opérations déjà faites sont annulées : le tableau, le fichier et l'historique d'annulation sont exactement comme avant. L'appel échoue (`isError`) avec l'**indice** et l'outil de l'opération en échec, son erreur, et les **résultats obtenus avant l'annulation** (pour voir ce qui se passait bien). Dans l'application, le lot entier compte sinon comme **une seule** étape d'annulation (Ctrl+Z).
- **`atomic: false`**. Le lot s'arrête à la première erreur et **conserve** ce qui a réussi (une seule étape d'annulation pour l'ensemble). La réponse est un résultat normal : `{"applied": 4, "failed": {"index": 4, "tool": "…", "error": "…"}, "results": [...]}`. L'opération en échec ne laisse elle-même rien derrière elle.
- Succès : `{"applied": n, "results": [<résultat de chaque opération>]}`.
- Les demandes mal formées (pas une liste, vide, plus de 200 opérations, clé inconnue dans une opération, outil interdit, clé interdite) sont refusées **avant** toute exécution.

### Sécurité

Le confinement à `--root` s'applique au `path` du lot. Une opération **ne peut pas porter** de clé nommée `path`, `output` ou `source` — ni à côté de `tool`, ni dans `args`, à quelque profondeur que ce soit : l'erreur est `paths cannot be set inside a batch operation; use the batch-level path`. Pour ajouter une image, utilisez `data_base64` (la limite de 15 Mio s'applique comme d'habitude). Les modifications faites par un lot sont celles de l'agent lui-même : `board_changes` ne les signale pas comme des modifications de l'utilisateur.

## Ressources et notifications

Le serveur déclare les capacités MCP `resources` (avec `subscribe` et `listChanged`) :

- `inkboard://live` : le tableau affiché dans l'application (même contenu que `describe_board`, avec la sélection) — présent seulement quand l'application est ouverte ;
- `file:///…/*.inkboard` (et `*.inkboard.json`, `*.tableau.json`) : les tableaux sous la racine `--root` (3 niveaux de dossiers, 300 fichiers au plus).

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
- Le protocole MCP est implémenté à la main (`initialize`, `ping`, `tools/list`, `tools/call`, `resources/*`) ; pas d'invites.
- Testé de bout en bout avec un client écrit pour l'occasion (`crates/inkboard-mcp/tests/session.rs`, `crates/inkboard-app/src/tests.rs`) **et avec Claude Code 2.1.258**
  (mode fichier, puis mode direct contre l'application réelle : ajout d'un rectangle, `describe_board` avec `live: true` et sélection). Les ressources, abonnements et notifications sont testés avec le vrai binaire et avec l'application (tests automatisés) mais **pas avec un client MCP réel qui les exploite** (Claude Code a servi pour les outils ; je n'ai pas pu vérifier ce qu'il fait des notifications). Pas testé avec Claude Desktop.
- `render` mesure le texte approximativement et, en mode direct, s'exécute sur le fil d'interface de l'application (une très grande image peut figer brièvement la fenêtre) ; l'application doit répondre en moins de 30 secondes.
- Un `batch` contient au plus 200 opérations ; ses résultats sont tous renvoyés dans une seule réponse (un très gros `describe_board` dans un lot donne une grosse réponse). En cas d'échec atomique, les résultats partiels cités sont coupés après 8 000 caractères. Les images ajoutées par une opération ensuite annulée peuvent rester en mémoire jusqu'au prochain enregistrement de l'application (le fichier n'est jamais écrit en cas d'échec).
- Plusieurs agents écrivant en même temps dans le même fichier (mode fichier) : le dernier gagne.
