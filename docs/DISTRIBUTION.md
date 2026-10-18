# Distribution

Le projet est libre, sous la double licence **MIT OU Apache-2.0** ([LICENSE-MIT](../LICENSE-MIT), [LICENSE-APACHE](../LICENSE-APACHE)). Les licences des composants tiers embarqués
(toutes permissives, dont la police STIX Two Math sous SIL OFL) sont dans [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md), généré par `python scripts/gen_third_party.py`
(à relancer quand les dépendances changent ; il échoue si une licence non permissive apparaît).

## Installeurs

Un installeur par système, produit par [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) à partir de [`Packager.toml`](../Packager.toml) :

| Système | Formats | État |
|---|---|---|
| Windows | `.exe` (NSIS) et `.msi` (WiX) | **construits et testés** : installation silencieuse, raccourci du menu Démarrer, entrée « Applications installées », lancement de l'application installée, désinstallation propre |
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

Le workflow [`release.yml`](../.github/workflows/release.yml) construit les installeurs sur Windows, Linux, macOS Intel et macOS Apple Silicon, calcule leurs empreintes SHA-256 et les attache à une
Release GitHub. Il peut aussi être lancé à la main (les installeurs restent alors des artefacts du workflow). **Ce workflow n'a jamais été exécuté** : le premier passage corrigera probablement des détails (versions de runners, dépendances Linux).

## Signature (facultative, mais recommandée pour une installation sans avertissement)

Sans signature, l'installation fonctionne, avec un avertissement : **SmartScreen** sous Windows (« Windows a protégé votre ordinateur » → *Informations complémentaires* → *Exécuter quand même*), **Gatekeeper** sous macOS
(clic droit → *Ouvrir*). Le workflow produit des installeurs non signés tant qu'aucun secret n'est configuré.

| Système | Option adaptée à un projet open source |
|---|---|
| Windows | **SignPath Foundation** : signature gratuite pour les projets open source publics (demande à faire sur signpath.org, validation du projet). Sinon **Azure Trusted Signing** (~10 $/mois, vérification d'identité) ou un certificat OV/EV payant. |
| macOS | Compte **Apple Developer** (99 $/an) : certificat « Developer ID » + notarisation. Renseigner les secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` ; `cargo-packager` signe et notarise. |
| Linux | Pas de signature de code obligatoire ; publier les empreintes SHA-256 (déjà fait) et, au besoin, signer les artefacts avec `cosign` ou GPG. |

## Archive portable Windows (sans installeur)

`powershell -File scripts/package.ps1` produit un `.zip` avec l'exécutable, la CLI, le serveur MCP et les scripts `install-user.ps1` / `uninstall-user.ps1`
(installation par utilisateur sans droits administrateur ; ces deux scripts n'ont jamais été exécutés).

## Icône

`crates/inkboard-app/assets/icon.png` (fenêtre) et `icon.ico` (exécutable, intégré par `build.rs` via `embed-resource` ; installeurs : voir `icons` dans `Packager.toml`).
