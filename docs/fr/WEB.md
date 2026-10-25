# Inkboard dans le navigateur

*English version: [WEB.md](../WEB.md)*

La même application tourne dans un navigateur : elle est compilée en WebAssembly (`wasm32-unknown-unknown`) et dessinée sur un `<canvas>`. La version web
est l'application locale (dessiner, éditer, annuler, bibliothèque d'icônes, ouvrir / télécharger des fichiers) **et un invité des sessions partagées** :
quelqu'un à qui l'on a envoyé un lien rejoint le tableau d'un hôte depuis la page, sans rien installer (voir
[Rejoindre une session depuis le navigateur](#rejoindre-une-session-depuis-le-navigateur)). Un navigateur ne peut pas héberger une session.

## Construire la version web

Il faut Rust 1.91 ou plus, la cible WebAssembly et [Trunk](https://trunkrs.dev) :

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli --locked
```

Puis, depuis la racine du dépôt :

```bash
scripts/build_web.sh          # Windows : .\scripts\build_web.ps1
```

Le script lance `trunk build --release` dans `crates/inkboard-app/web/` et écrit la page statique dans **`crates/inkboard-app/web/dist/`** :
`index.html`, un fichier `.js`, un fichier `.wasm` et `icons.bin` (la bibliothèque d'icônes, 8 Mo). N'importe quel serveur web statique peut héberger ce
dossier (les liens sont relatifs) ; pour l'essayer en local, lancez `trunk serve` dans `crates/inkboard-app/web/` et ouvrez <http://127.0.0.1:8080>.

- **Rendu** : WebGL (`glow`, par défaut, plus léger). `scripts/build_web.sh wgpu` (Windows : `-Renderer wgpu`) construit le rendu WebGPU avec repli WebGL
  (fonctionnalité `web-wgpu`, un peu plus lourd).
- **Chaîne d'outils** : si le Rust choisi par `rustup` est plus ancien que 1.91, définissez `RUSTUP_TOOLCHAIN` (par exemple `RUSTUP_TOOLCHAIN=1.99.1`).
- **Compilateur C** : le transport réseau (`iroh`, via `ring`) compile du C pour WebAssembly ; la construction exige un `clang` avec la cible WebAssembly (LLVM 15 ou plus ; sous Linux, le paquet `clang`). Si ce n'est pas le `clang` du `PATH`, définissez `CC_wasm32_unknown_unknown` et `AR_wasm32_unknown_unknown` vers un `clang` et un `llvm-ar`.
- La version de bureau n'est pas touchée : `cargo build -p inkboard-app` est inchangé et embarque toujours le pack d'icônes.

## Ce qui diffère de l'application de bureau

| | Bureau | Navigateur |
|---|---|---|
| Bibliothèque d'icônes | embarquée dans le programme | `icons.bin` est téléchargé à la première ouverture de la bibliothèque (un indicateur de chargement s'affiche), puis mis en cache par le navigateur |
| Ouvrir | boîte de dialogue | le sélecteur de fichiers du navigateur, ou déposer une image sur la page |
| Enregistrer / Exporter | écrit le fichier | **télécharge** le fichier |
| Presse-papiers | presse-papiers système | coller du texte et des images avec Ctrl+V ; « copier comme image » passe par le presse-papiers asynchrone du navigateur (pages sécurisées seulement : HTTPS ou localhost) ; le menu « Coller » ne colle que le presse-papiers interne du tableau |
| Sauvegarde automatique, réglages, raccourcis personnalisés | fichiers du dossier de données | **non conservés** d'une visite à l'autre (téléchargez le tableau pour le garder) |
| Polices | polices du système | polices intégrées (le texte peut se replier autrement que sur un tableau de bureau) |
| Pression du stylet, gomme | oui (Windows) | non (les événements de pointeur du navigateur ne portent pas la pression ici) |
| Agent MCP (mode direct), surveillance du fichier | oui | non |
| Héberger une session | oui | jamais (la version web ne peut que rejoindre) |
| Rejoindre une session | lien ou menu, connexion directe quand c'est possible | lien ou menu, **par le relais seulement** (une page ne peut pas ouvrir de connexion directe) |

## Rejoindre une session depuis le navigateur

**Le lien** est l'adresse de la page suivie de `#` et du ticket : `https://<adresse de la page>/#<ticket>`. `<ticket>` est exactement la partie située après
`inkboard://join#` (ou `inkboard://join/`) du lien que donne l'application de l'hôte. La page lit aussi `#join=<ticket>`. Le ticket est dans le fragment (après
`#`) : un navigateur ne l'envoie jamais à un serveur, donc l'hébergeur web de la page ne le voit jamais. Dès que la page l'a lu, elle **l'efface de la barre
d'adresse** (`history.replaceState`) : il ne reste ni dans l'adresse ni dans l'entrée d'historique. La page est publiée sur
`https://victorletirand.github.io/inkboard/`, et l'application fabrique le lien web pour vous : dans la fenêtre Partager, **Copier le lien web** (la variable
d'environnement `INKBOARD_WEB_URL` permet de viser une autre adresse, par exemple une page servie sur un réseau local). On peut aussi donner à l'invité
l'adresse de la page et le lien natif.

**Ce que voit l'invité :**

1. La page se charge (barre de progression ; environ 5 Mo la première fois, ensuite en cache).
2. Si l'adresse contenait un ticket, la fenêtre **Rejoindre une session** s'ouvre avec le lien (« Lien reçu ») et le champ **nom**, déjà sélectionné ; le nom de la
   dernière visite est pré-rempli (gardé dans le stockage local du navigateur sous `inkboard.display_name` : cet appareil seulement) et nettoyé comme sur le bureau
   (caractères de contrôle, espaces en trop, 40 caractères). Sans ticket dans l'adresse : menu principal, **Rejoindre une session…**, puis coller un lien ou un ticket
   (`inkboard://join#…`, `https://…#…` ou le ticket seul). Un lien invalide ouvre la fenêtre avec la raison et le texte à corriger. Le nom de l'hôte n'est pas dans le
   ticket : il apparaît quand l'hôte a répondu (« Session de *nom* »).
3. **Rejoindre**, puis l'accord de l'hôte (« En attente de l'accord de l'hôte… », puis le tableau). L'invité modifie, et voit les curseurs et les noms des autres
   participants et le curseur de l'agent MCP, comme un invité installé.
4. **États et erreurs** : ceux de l'application, traduits (français / anglais) : connexion, « Reconnexion… » après environ 2,5 s sans nouvelle (modifications
   conservées et envoyées plus tard), refus de l'hôte, session pleine, lien qui n'est plus valable, hôte parti (avec **Enregistrer une copie du tableau**) et, quand le relais
   est injoignable (proxy d'entreprise, réseau scolaire), le message avec ses **trois pistes** et **Copier le diagnostic** (presse-papiers du navigateur). Une page ne peut
   ni sonder le relais ni lire le 403 d'une passerelle : elle sait seulement si sa connexion WebSocket a atteint le relais ; le message est donc générique, mais les pistes
   sont les mêmes.
5. **Reprise rapide** : la page écoute l'événement `online` du navigateur et `visibilitychange` (onglet de nouveau visible : un onglet caché ne dessine rien, la session a
   donc dormi) et se reconnecte tout de suite, sans attendre ses pauses entre tentatives.
6. **Images** : Ctrl+V d'une image du presse-papiers et un fichier déposé sur la page sont insérés et envoyés à l'hôte par morceaux, acquittés, avec les mêmes limites que
   les invités de l'application. « Copier comme image » écrit un PNG dans le presse-papiers du navigateur (HTTPS ou localhost seulement). **Enregistrer** (Ctrl+S) et les
   entrées d'export (PNG, SVG, PDF…) **téléchargent** une copie ; il n'y a pas de boîte de dialogue de fichier.
7. **Téléphone et tablette** : la page a un viewport qui ne zoome pas tout seul ; dans la fenêtre Rejoindre et le bandeau de session, boutons et champs sont à taille tactile
   (40 px) quand le pointeur principal est un doigt, et les fenêtres tiennent dans la largeur de l'écran. Le reste de l'interface (barre d'outils, panneaux) est inchangé.

## Navigateurs

Essayé, avec le banc ci-dessous (65 vérifications et plus ; un vrai hôte natif et le relais public), sous Windows 11, sans fenêtre (headless) :

| Moteur | Résultat |
|---|---|
| **Chrome 155** (et un téléphone émulé par Chrome) | toutes les vérifications passent |
| **Edge 155** (Chromium) | toutes les vérifications passent |
| **Firefox 158** (version officielle, pilotée par WebDriver BiDi) | toutes les vérifications passent ; 2 ne peuvent pas être faites (les événements `online` / `offline` : la coupure passe par un proxy et l'invité reprend par ses propres tentatives) |
| **WebKit** (build de Playwright pour Windows, **une approximation de Safari, pas Safari**) | toutes les vérifications passent ; 4 ne peuvent pas être faites avec ce build (il refuse toute écriture d'image dans le presse-papiers, l'automatisation ne peut pas le lire, un onglet en arrière-plan reste visible) |

La page publique (`https://victorletirand.github.io/inkboard/`) démarre et dessine aussi sur les quatre moteurs, de même qu'une copie servie en `http://` simple sur une adresse du réseau
local (contexte non sécurisé : elle démarre et dessine ; le presse-papiers n'y est pas disponible, comme dit plus haut).
**Non essayé : Safari lui-même, iOS / iPadOS, vrais téléphones ou tablettes** (clavier virtuel, pincement, latence tactile réelle), une vraie passerelle d'entreprise. Différences
constatées : Firefox ne donne aucun fichier à un événement de collage fabriqué par un script (un vrai Ctrl+V d'image fonctionne) ; dans un build WebKit, « Copier comme image » a été
refusé par le navigateur alors que l'application affichait « Image copiée » (à vérifier sur un vrai Safari) ; le moteur de Safari écrit dans la console une clé de viewport inconnue
(`interactive-widget`), sans conséquence. Une page qui ne peut pas démarrer le dit au lieu de rester blanche : ni WebAssembly, ni WebSocket, ni `crypto.getRandomValues`
(« ce navigateur est trop ancien »), ni WebGL 2 ni WebGPU (« pas de WebGL : activez l'accélération matérielle… »).

## Limites connues (invité navigateur)

- Pas d'hébergement ; relais seul (pas de chemin direct) : la latence est celle du relais (~50 ms sur le relais public) et la limite de débit des relais publics s'applique.
- Le domaine du relais doit être joignable depuis le réseau du navigateur (voir [SHARING.md](SHARING.md#réseaux-restreints-entreprise-école)).
- Seul le nom affiché de l'invité est gardé d'une visite à l'autre ; ni les réglages ni la sauvegarde automatique.
- Pas de pression du stylet ; l'entrée de menu « Coller » ne colle que le presse-papiers interne du tableau (Ctrl+V fonctionne pour les images et le texte).
- La page pèse 18 Mo décompressée (4,7 Mo en Brotli, 7,7 Mo en gzip) ; la bibliothèque d'icônes (8 Mo) n'est téléchargée qu'à son ouverture.
- Un onglet caché ne dessine pas, donc ne traite pas la session tant qu'il n'est pas visible (c'est le navigateur qui décide ; la session rattrape son retard au retour).

## Tester la page

Deux scripts, lancés à la main (jamais par `cargo test`) : `page.mjs` (la page locale) et `e2e.mjs` (**un vrai hôte natif et des invités navigateur à travers le relais
public ; il faut Internet**).

`crates/inkboard-app/web/test/page.mjs` charge la page construite dans un Chrome installé (`puppeteer-core`), crée une forme, tape du texte, ouvre la
bibliothèque d'icônes (et vérifie que le pack n'est demandé qu'à ce moment), télécharge puis rouvre un tableau, colle et dépose une image, vérifie que deux
sessions ne produisent jamais les mêmes identifiants d'objets, et enregistre des captures dans `web/test/results/` :

```bash
cd crates/inkboard-app/web/test
npm install
node page.mjs            # CHROME=<chemin> si Chrome n'est pas à son emplacement Windows par défaut
node sizes.mjs ../dist   # tailles brute / gzip / brotli
```

Évitez de laisser `node_modules` dans l'arborescence du dépôt : le test du dépôt qui cherche du français dans les sources parcourt tous les dossiers.

`e2e.mjs` pilote Chrome (ou `--browser edge|firefox|webkit`) par la vraie interface et vérifie : la progression du chargement, les messages « trop ancien » et « pas de WebGL », un lien avec le ticket dans le
fragment (fenêtre du nom, rejoindre, instantané, ticket effacé de la barre d'adresse), un objet dessiné dans le navigateur qui arrive chez l'hôte, des images collées et
déposées qui arrivent chez l'hôte, le téléchargement d'une copie, deux invités navigateur qui voient les curseurs et les noms l'un de l'autre, le curseur de l'agent MCP, une
coupure réseau avec modifications conservées puis reprises, un lien en lecture seule, un lien invalide, un relais injoignable (message d'entreprise, trois pistes, diagnostic
copié, en anglais et en français), un téléphone émulé (cibles tactiles, focus du clavier virtuel, rejoindre au doigt), le départ de l'hôte. Il enregistre une capture de
chaque état dans `--out` (`context/mcp-remote/screenshots/f3/`) :

```bash
export PUPPETEER_DIR=<dossier de puppeteer-core>      # hors de l'arborescence du dépôt
export CC_wasm32_unknown_unknown=<clang>  AR_wasm32_unknown_unknown=<llvm-ar>   # pour ring, comme ci-dessus
node crates/inkboard-app/web/test/e2e.mjs [--browser chrome|edge|firefox|webkit] [--skip-build] [--out <dossier>] [--headed]
# Firefox : FIREFOX=<firefox.exe> ; WebKit : PLAYWRIGHT_DIR=<playwright-core> PLAYWRIGHT_BROWSERS_PATH=<dossier> ; EDGE=<msedge.exe>
node crates/inkboard-app/web/test/smoke-public.mjs --browser firefox     # la page publiée seulement
```
