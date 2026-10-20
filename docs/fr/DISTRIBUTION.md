# Distribution
*English version: [DISTRIBUTION.md](../DISTRIBUTION.md)*

Le projet est libre, sous la double licence **MIT OU Apache-2.0** ([LICENSE-MIT](../../LICENSE-MIT), [LICENSE-APACHE](../../LICENSE-APACHE)). Les licences des composants tiers embarqués
(toutes permissives, dont la police STIX Two Math sous SIL OFL) sont dans [THIRD_PARTY_LICENSES.md](../../THIRD_PARTY_LICENSES.md), généré par `python scripts/gen_third_party.py`
(à relancer quand les dépendances changent ; il échoue si une licence non permissive apparaît).
Le test `third_party_licenses_list_every_crate_embedded_in_the_binaries` la vérifie hors ligne : lancer d'abord `cargo fetch --locked` pour télécharger les crates de toutes les plateformes livrées (la CI le fait).

## Installeurs

Un installeur par système, produit par [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) à partir de [`Packager.toml`](../../Packager.toml) :

| Système | Formats | État |
|---|---|---|
| Windows | `.exe` (NSIS) et `.msi` (WiX) | **construits et testés** : installation silencieuse, raccourci du menu Démarrer, entrée « Applications installées », lancement de l'application installée, désinstallation propre (voir ci-dessous) |
| Linux | `.deb`, `.AppImage` (et PKGBUILD) | configurés, **jamais construits ni lancés** |
| macOS | `.dmg` (et `.app`), Intel et Apple Silicon | configurés, **jamais construits ni lancés** |

L'installeur installe l'application, la CLI de conversion `ink` et le serveur MCP `inkboard-mcp` côte à côte, ainsi que README et licences. En local (Windows) :

```bash
cargo install cargo-packager --locked
cargo build --release -p inkboard-app -p inkboard-cli -p inkboard-mcp
cargo packager --release -c Packager.toml --formats nsis     # ou wix
```

Les installeurs sont écrits dans `dist/`. La version vient de `Packager.toml` : elle doit rester égale à celle de `Cargo.toml` (un test le vérifie).

### Publier une version (GitHub Releases)

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Le workflow [`release.yml`](../../.github/workflows/release.yml) construit les installeurs sur Windows, Linux, macOS Intel et macOS Apple Silicon, calcule leurs empreintes SHA-256 et les attache à une
Release GitHub. Il peut aussi être lancé à la main (les installeurs restent alors des artefacts du workflow). **Ce workflow n'a jamais été exécuté** : le premier passage corrigera probablement des détails (versions de runners, dépendances Linux).

## L'installation Windows en détail

L'installeur `.exe` est celui à recommander :

- **Par utilisateur, sans demande d'administrateur.** Il s'installe dans `%LOCALAPPDATA%\Inkboard`, ajoute un raccourci au menu Démarrer et une entrée dans *Paramètres → Applications* (avec éditeur, version et icône) ; son désinstalleur retire tout ce qu'il a installé. Vos tableaux ne sont jamais touchés ; le désinstalleur propose (décoché par défaut) de supprimer le dossier de données `%APPDATA%\Inkboard` (raccourcis personnalisés, langue, sauvegardes automatiques).
- **Association de fichier.** `Packager.toml` déclare l'extension `.inkboard` (`[[file-associations]]`, type MIME `application/x-inkboard`) : les installeurs l'enregistrent pour qu'un double-clic lance `inkboard <fichier>` ; le `.deb` installe aussi une définition shared-mime-info et le paquet macOS la déclare dans `CFBundleDocumentTypes`. **Pas encore vérifié** : aucun installeur n'a été lancé depuis l'ajout. Les fichiers enregistrés par les versions précédentes (`.inkboard.json`, `.tableau.json`) ne sont pas associés (`.json` appartient à d'autres programmes) mais s'ouvrent toujours depuis l'application. Limite connue de cargo-packager 0.11.8 : son modèle WiX lit `association.ext` alors que le champ s'appelle `extensions`, donc le `.msi` risque de ne pas enregistrer l'association (le `.exe` le fait). `inkboard_*_x64-setup.exe /S` (et `uninstall.exe /S` dans le dossier d'installation).
- **En anglais ou en français**, selon la langue de Windows.
- **Autonome.** Les exécutables lient statiquement la bibliothèque C et n'importent que des bibliothèques système de Windows : rien à installer au préalable (pas de Visual C++ redistribuable). Ils portent une icône, des informations de version et un manifeste d'application (pas d'élévation, UTF-8, chemins longs, DPI par écran), et l'application est un programme graphique : aucune fenêtre de console ne s'ouvre derrière elle.
- **Fonctionne sans bonne carte graphique.** L'application utilise OpenGL et bascule sur Direct3D 12 / Vulkan (avec un repli logiciel) quand OpenGL manque ou est trop ancien (bureau à distance, machines virtuelles). `INKBOARD_RENDERER=wgpu` (ou `glow`) en force un. Si la fenêtre ne peut pas s'ouvrir, un message explique pourquoi ; si l'application plante, elle écrit `%APPDATA%\Inkboard\crash.log` et le dit.

Le `.msi` est le paquet Windows Installer classique (pour toute la machine, droits d'administrateur requis), pour les environnements gérés. Il a été moins éprouvé que le `.exe`.

### winget

`winget install Inkboard.Inkboard` suppose que le paquet soit accepté dans [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). Les modèles de manifestes sont dans [`packaging/winget`](../../packaging/winget) ; pour préparer la première soumission quand une version est publique :

```powershell
powershell -File scripts/winget-manifest.ps1 -Tag v0.1.0              # télécharge l'installeur, calcule son SHA-256, écrit dist/winget/manifests/...
winget validate --manifest dist/winget/manifests/i/Inkboard/Inkboard/0.1.0
winget settings --enable LocalManifestFiles
winget install --manifest dist/winget/manifests/i/Inkboard/Inkboard/0.1.0   # l'essayer
wingetcreate submit dist/winget/manifests/i/Inkboard/Inkboard/0.1.0         # ouvre la pull request (jeton GitHub requis)
```

Une fois la première version fusionnée, le job `winget` du workflow de publication soumet chaque étiquette stable automatiquement si le secret `WINGET_TOKEN` (un jeton d'accès personnel classique avec la portée `public_repo`) est défini. winget n'applique pas la marque « téléchargé depuis Internet » : une installation par winget n'affiche jamais l'avertissement SmartScreen ; la chaîne de validation de winget-pkgs analyse en revanche l'installeur, une raison de plus de le signer.

## Signature (recommandée : c'est elle qui supprime l'avertissement Windows)

Sans signature, l'installation fonctionne, avec un avertissement : **SmartScreen** sous Windows (« Windows a protégé votre ordinateur » → *Informations complémentaires* → *Exécuter quand même*), **Gatekeeper** sous macOS
(clic droit → *Ouvrir*). Aucun empaquetage ne supprime cette fenêtre : seul un certificat de signature de code le fait, et il est délivré à une identité vérifiée ;
c'est donc un compte à créer, pas quelque chose que le dépôt peut contenir. Le workflow de publication est prêt : **il signe automatiquement dès que les secrets
sont définis, et produit des installeurs non signés sinon.**

| Système | Option adaptée à un projet open source |
|---|---|
| Windows | **Azure Artifact Signing** (ex Trusted Signing), environ 10 $/mois, vérification d'identité ; géré par le workflow (ci-dessous). **SignPath Foundation** : signature gratuite pour les projets open source publics (demande sur signpath.org ; le projet est examiné ; il faut un dépôt public et que les installeurs passent par les artefacts GitHub, ce que le quota de stockage du compte gratuit rend juste). Un certificat OV/EV classique marche aussi avec `signtool`. |
| macOS | Compte **Apple Developer** (99 $/an) : certificat « Developer ID » + notarisation. Renseigner les secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` ; `cargo-packager` signe et notarise. |
| Linux | Pas de signature de code obligatoire ; publier les empreintes SHA-256 (déjà fait) et, au besoin, signer les artefacts avec `cosign` ou GPG. |

### Mettre en place Azure Artifact Signing pour Windows

1. Dans le portail Azure, créer un **compte Artifact Signing** et terminer la **validation d'identité** (particulier ou organisation ; Microsoft décide qui est éligible dans quelle région).
2. Créer un **profil de certificat** (type *Public Trust*).
3. Créer une **inscription d'application** (un principal de service) et lui donner le rôle *Artifact Signing Certificate Profile Signer* sur le compte.
4. Dans les paramètres du dépôt GitHub, ajouter les **secrets** `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` et les **variables** `SIGNING_ENDPOINT` (le point d'accès régional du compte, par exemple `https://weu.codesigning.azure.net`), `SIGNING_ACCOUNT` et `SIGNING_PROFILE`.
5. Pousser une étiquette : les exécutables (`inkboard.exe`, `ink.exe`, `inkboard-mcp.exe`) sont signés et horodatés avant l'empaquetage, puis les installeurs `.exe` et `.msi` sont signés.

Une application tout juste signée peut encore déclencher SmartScreen un moment : sa réputation se construit avec le nombre de téléchargements, et la signature est ce qui lui permet de la construire au lieu de repartir de zéro à chaque version.

## Archive portable Windows (sans installeur)

`powershell -File scripts/package.ps1` produit un `.zip` avec l'exécutable, la CLI, le serveur MCP et les scripts `install-user.ps1` / `uninstall-user.ps1`
(installation par utilisateur sans droits administrateur ; ces deux scripts n'ont jamais été exécutés).

## Icône

`crates/inkboard-app/assets/icon.svg` (source, dessin 180 × 180), `icon.png` (fenêtre) et `icon.ico` (exécutable, intégré par `build.rs` via `embed-resource` ; installeurs : voir `icons` dans `Packager.toml`).
