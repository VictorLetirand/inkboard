# Inkboard dans le navigateur

*English version: [WEB.md](../WEB.md)*

La même application tourne dans un navigateur : elle est compilée en WebAssembly (`wasm32-unknown-unknown`) et dessinée sur un `<canvas>`. La version web
est pour l'instant l'application locale seule (dessiner, éditer, annuler, bibliothèque d'icônes, ouvrir / télécharger des fichiers) ; rejoindre une
session partagée viendra avec le transport web et n'est pas encore branché.

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

Cibles : versions récentes de Chrome, Edge, Firefox et Safari. La page exige WebGL 2 (ou WebGPU dans la version `web-wgpu`) : sans lui, un message explique
quoi faire.

## Tester la page

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
