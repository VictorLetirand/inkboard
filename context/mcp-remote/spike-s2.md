# Spike S2 — Inkboard dans un navigateur (WASM)

*2026-10-21, branche `feat/remote-s2` (depuis `feat/remote` 52f838b). Reprendre uniquement `spikes/s2-wasm/` et ce fichier ; les modifications de `crates/` sont des `cfg` provisoires restées sur la branche S2.*

## RECOMMANDATION : stratégie A (cfg-gating de `inkboard-app`), avec une couture « plateforme » propre

**Le port tourne déjà** : `inkboard-app` compile en `wasm32`, affiche un tableau, des formes, crée des formes au clic / tactile / stylet, reçoit un fichier déposé (octets), détecte le français. Le gating nécessaire est de **~220 lignes ajoutées / 33 retirées sur 14 fichiers + ~150 lignes neuves** (`web.rs`, `time.rs`), obtenu en moins d'une heure de travail effectif. Le plan estimait « une vingtaine de points natifs » : en réalité **5 familles de blocages réels** (tableau ci-dessous), le reste compile tel quel.

**Estimation globale du lot F avec A : 6 à 9 jours-agent** (T1) : persistance web (S), dialogues/téléchargement (M), presse-papiers image (M), polices embarquées communes (M, déjà prévu lot H), stylet avec pression (M), transport web et UI de session (hors S2), CI + publication (S), tests `wasm-bindgen-test` (M), essais manuels navigateurs (S).
**Pourquoi pas B** : extraire un crate d'interface partagé déplacerait ~11 500 lignes (dont ~3 400 de tests qui font `crate::…` et dont les tests i18n lisent `src/` via `CARGO_MANIFEST_DIR`) pour un gain nul aujourd'hui : les points natifs restants sont ~25 sites isolés. Estimation B : +8 à 12 jours, risque de conflits avec les lots A/C/D qui touchent `app.rs`. **À revoir seulement si** un second client (mobile, autre shell) apparaît. Parade à A : regrouper les 5 familles derrière un petit module `platform` (presse-papiers, fichiers, horloge, persistance, polices) pour que le `cfg` ne se disperse pas.

## Versions

rustc 1.90.0 (projet), cible `wasm32-unknown-unknown` ; trunk 0.22.0 (exige rustc ≥ 1.96 : installé avec `cargo +1.99.1 install trunk --locked`) ; wasm-bindgen(-cli) 0.2.129 ; wasm-opt version_131 (téléchargé par trunk) ; eframe/egui/egui-wgpu 0.33.3, wgpu 27.0.1, web-sys 0.3.106 ; twiggy ; puppeteer-core 24.43.1 ; Chrome 155.0.8059.80 (GPU NVIDIA RTX 5070, Windows 11) ; node 24.14.1.

## Étape 2 : crates purs

`cargo build --target wasm32-unknown-unknown` de `inkboard-core`, `inkboard-icons`, `inkboard-formats` **et** `inkboard-mcp` (lib) : **compilent sans aucune modification** (25 s / 16 s / 53 s à froid, build dev). Pas de souci `getrandom`. Deux pièges d'exécution, pas de compilation :
- `inkboard-core::model::next_seed` utilise une graine **constante** en `wasm32` : deux invités web généreraient les **mêmes `Id`** (collision en session). Il faut appeler `seed_ids(aléatoire)` au démarrage (fait dans `web.rs` avec `Math.random` ; à remplacer par `crypto.getRandomValues`).
- `std::time::Instant::now()` / `SystemTime::now()` **paniquent** sur `wasm32-unknown-unknown` (voir tableau).

## Étape 3 : blocages de `inkboard-app` en wasm32

| Module / fonction | Cause | Effort | Solution (état dans le spike) |
|---|---|---|---|
| `arboard` (`clipboard.rs`) | ne compile pas en wasm | M | dépendance réservée au natif ; web : texte via `Event::Paste` d'eframe (déjà géré) ; image : écouteur `paste` propre + `ctx.copy_image` pour copier (stub dans le spike) |
| `rfd::FileDialog` (ouvrir, enregistrer, exporter, insérer image : 4 sites) | API synchrone absente en web | M | `rfd::AsyncFileDialog` (octets) + téléchargement par Blob ; stubs « non câblé » dans le spike |
| `std::time::Instant` (~40 sites, 6 fichiers) | panique à l'exécution | S | alias `crate::time` → `web-time` (fait, remplacement mécanique) |
| Point d'entrée `main.rs` (`run_native`, `report` via `rfd`, panic hook + `crash.log`, `windows_subsystem`) | natif seulement | S | `main` wasm → `web::start()` + `WebRunner` ; panic géré par eframe (fait) |
| `eframe` : un seul peintre web à la compilation | si `glow` est actif il écrase `wgpu` | S | `default-features = false` côté wasm, features `web-glow` / `web-wgpu` (fait) |
| Pack d'icônes (`include_bytes!` via `inkboard-mcp` → `inkboard-icons/embedded`) | 8,2 Mo dans le `.wasm` | S | feature `embedded-icons` (défaut natif) ; web : `fetch("icons.bin")` à la demande à l'ouverture de la palette (fait, testé) |
| Fichiers (`std::fs` : autosave, keymap, langue, `open_path`, `watch.rs`) | compile, échoue en `Unsupported` sans panique | M | persistance `localStorage`/IndexedDB (autosave, raccourcis, langue) ; `watch.rs` à désactiver (spike : inactif de fait) |
| Dépôt de fichiers (`dropped_files`, `insert_image_files`) | lisait `f.path` seulement (rien ne se passait en web) | S | `insert_image_bytes` + lecture de `f.bytes` (fait, vérifié) |
| Polices système (`fonts.rs`) | Segoe/Georgia/Consolas absentes ; non redistribuables | M | jeu embarqué commun (lot H) ; le spike retombe sur les polices par défaut d'egui (retours à la ligne différents) |
| `sys-locale` | sans feature `js` renvoie `None` | S | feature `js` (fait : l'interface s'affiche en français) |
| `viewport commands` (Title, Close) | « not implemented in web backend » (avertissement) | S | `document.title` à la main |
| `live.rs` (TcpListener, threads), `inkboard-mcp` live | compile ; `bind` échoue, pas de `spawn` atteint | S | ne pas démarrer (`LiveServer` jamais créé en web) ; en lot F : `cfg` explicite |
| `inkboard-pen` (Windows) | no-op hors Windows, compile tel quel | S | stylet web : pression/gomme via `PointerEvent` (voir §5) |
| `raw-window-handle`, `embed-resource` (build.rs) | seulement natif | S | rien (build.rs sans effet hors Windows) |
| Tests (`egui_kittest`, i18n qui lisent `src/`) | natifs | S | restent natifs ; non touchés |

## Étape 4 : mesures (release, `trunk build --release`, wasm-opt `-Os`)

| Variante | `.wasm` brut | gzip -9 | brotli 11 |
|---|---|---|---|
| glow, sans pack | 12,31 Mo | 5,09 Mo | 3,44 Mo |
| glow, **avec** pack embarqué | 20,50 Mo | 13,27 Mo | 11,63 Mo |
| wgpu (WebGPU + WebGL), sans pack | 15,23 Mo | 6,05 Mo | 4,12 Mo |
| glow « small » (`opt-level=z`, LTO fat, `panic=abort`, via variables d'environnement) | 7,95 Mo | 3,61 Mo | 2,79 Mo |

`icons.bin` à part : 8,18 Mo (déjà compressé, ne gagne rien à gzip). Page glow sans pack + `icons.bin` à la demande : 5,1 Mo gzip au chargement (budget du plan < 15 Mo : tenu largement).
Le pack embarqué coûte +8,2 Mo brut / +8,2 Mo gzip : à charger à la demande, comme prévu.

**FPS, tableau de 2 000 icônes** (`?iconboard=2000`, banc scripté de l'application `IconBench`, mêmes phases que `--bench-icons`, écran ~180 Hz) : natif (release, glow) et web (glow, wgpu/WebGPU, wgpu/WebGL) **tous à l'intervalle de rafraîchissement (p50 5,6 ms)** ; fraction de trames > 16,7 ms : 0 % partout sauf 0,7 % pendant le panoramique (pic isolé 24–31 ms web). CPU d'`update` : natif 1,2 – 2,6 ms (moyennes par phase), web 1,2 – 4,4 ms. Rastérisation SVG de la 1re vue : natif 817 icônes en 49 ms, web ~1 050 en 103–116 ms (~2× plus lent) ; variante « small » : 206 ms. Textures : 76,6 Mo web, 111,6 Mo natif. **Le journal `usvg` vers la console fausse tout** (CPU moyen ×2) : `web.rs` journalise seulement les erreurs par défaut.
**Démarrage** : mesure **non fiable** sur cette machine chargée (par exemple glow : de 0,1 s à 12 s selon les lancements ; série calme : médiane ~0,3 s glow, ~0,75 s avec pack, ~0,9 s WebGPU, ~0,35 s WebGL, depuis `localhost`, donc sans temps de téléchargement). Création de l'application : 20–60 ms (glow), ~330 ms (WebGPU, initialisation de l'adaptateur).

## Comparaison des rendus

| | glow (WebGL2) | wgpu → WebGL (`Backends::GL`) | wgpu → WebGPU |
|---|---|---|---|
| Rendu (captures 1360×860) | identique | identique | identique |
| `.wasm` brotli | 3,44 Mo | 4,12 Mo (le même binaire contient les deux) | idem |
| Backend confirmé | — | `backend: Gl`, ANGLE/RTX 5070 | `backend: BrowserWebGpu` (adaptateur anonyme) |
| FPS 2 000 icônes | vsync | vsync | vsync |
Constat : **à ce niveau de charge rien ne les départage** ; `glow` est plus léger de ~0,7 Mo brotli (−21 %) et démarre plus vite, sans dépendre de WebGPU (absent de Firefox stable/Safari anciens). `wgpu` n'apporte pas de gain visible. **Choix : `glow` par défaut**, wgpu (par `?renderer=`) reste possible sans autre code que `web.rs`. Test fait avec 2 000 icônes seulement (pas 5 000).

## Étape 5 : entrées web (code d'eframe 0.33.3 lu, puis essais)

- **Presse-papiers** : le drapeau `--cfg=web_sys_unstable_apis` **n'est pas nécessaire** ici : eframe 0.33.3 + web-sys 0.3.106 compilent sans (`Clipboard::write/read` à valeur `JsValue` sont stables ; seules les variantes typées sont derrière le drapeau). Copier/couper/coller du **texte** : `Event::Copy/Cut/Paste` (écouteurs `copy/cut/paste`, exige un contexte sécurisé HTTPS/localhost). Copier une **image** : `ViewportCommand::CopyImage` marche en web. **Coller une image : aucun événement egui** (le texte seul est lu) → écouteur `paste` propre sur le document, lecture de `clipboardData.files`, passage de l'image à `App` (M).
- **Dépôt de fichiers** : eframe lit chaque fichier (`File.arrayBuffer`) et fournit `DroppedFile{name, mime, bytes, path: None}`. Corrigé dans `app.rs` ; **vérifié** (PNG synthétique déposé : image insérée et sélectionnée).
- **Tactile** : `touchstart/move/end` → `Event::Touch{force}` ; **vérifié** (un tap crée une note). Pincer/zoomer : gestes Safari et molette gérés par eframe, non essayés.
- **Stylet** : `pointerdown/up` (+ `mousemove`) ; eframe **ne lit ni `pressure`, ni `pointerType`, ni le bouton gomme** ; `pen_force` d'Inkboard n'est alimenté que par `Event::Touch.force`. **Vérifié** : trait de stylo (CDP `pointerType: pen`) dessiné ; pression non exploitée → traits à épaisseur constante. Parade : écouteur `pointermove` propre (pression, `buttons & 32` = gomme) → M.
- **Clavier / IME** : raccourcis `r`, `n`, `p`, saisie « hello » dans une note : OK. Mobile (clavier virtuel) : non vérifié.

## Faits à retenir pour le lot F

`WebRunner` exige un `<canvas>` ; titre de fenêtre, fermeture, thème système : à régler à la main ; `accesskit` retiré en web (`default-features=false`) ; Id des objets à ensemencer (ci-dessus) ; `Library::open` veut un `&'static [u8]` (le pack téléchargé est « fuité » volontairement).

## Reproduction

`spikes/s2-wasm/` : `build.ps1` (ou directement `trunk build --no-default-features --features web-glow --dist dist-glow --release`), `trunk serve` possible (`Trunk.toml`, port 8080), `sizes.mjs` (poids brut/gzip/brotli), `drive.mjs` (Chrome piloté par puppeteer-core : `--interact`, `--drop`, `--touch`, `--bench`, captures). `npm install` d'abord. Paramètres de page : `?demo=shapes`, `?renderer=webgl|webgpu` (build wgpu), `?iconboard=2000`, `?log=debug`.

## NON vérifié

- Firefox, Safari, Edge (le lancement piloté d'Edge a échoué ; même moteur que Chrome, non prouvé), mobile réel, vrai écran tactile ou stylet physique (événements synthétiques seulement), pression du stylet.
- Les captures ont été regardées par l'agent S2 (Chrome sans fenêtre visible) ; l'agent principal doit ouvrir la page (`trunk serve`) pour un contrôle visuel humain. L'adaptateur WebGPU est anonyme : matériel ou logiciel inconnu.
- Démarrage chiffré fiable ; réseau (téléchargement de 5 Mo), cache HTTP du `.wasm`, compression côté serveur.
- Tableau de 5 000 icônes ; très gros tableaux ; mémoire wasm (limite 4 Go, 76,6 Mo de textures à 2 000 icônes).
- Ouvrir/enregistrer/exporter/insérer par dialogue, coller une image, persistance, `watch.rs`, transport de session : non câblés.
- Tests natifs (`cargo test`, i18n), `clippy`, `fmt` : **non relancés** sur les `cfg` provisoires ; seul `cargo build --release -p inkboard-app` natif a réussi.
- Estimations de jours (A : 6–9 j, B : +8–12 j) : jugement, pas mesure.
