# Lot K — kit de relais et documentation d'utilisation du partage

Livrés : `docs/SHARING.md`, `docs/fr/SHARING.md`, `docs/RELAY.md`, `docs/fr/RELAY.md`, `packaging/relay/` (compose, `relay.toml`, README), courtes sections dans `README.md` / `README.fr.md`. Aucun code.

## Vérifié
- Libellés des boutons lus dans `share_ui.rs`, `join_ui.rs`, `chrome.rs`, `i18n_fr.rs` (branche `feat/remote`, 35d806d).
- Options de `relay.toml` lues dans la source d'`iroh-relay` 1.4.0 (registre cargo) ; page « Self-hosted » et « Rate limiting » d'iroh ; image `n0computer/iroh-relay:v1.4.1` (entrypoint `/iroh-relay`, vu via le registre Docker Hub).
- `docker compose config` et un analyseur TOML : syntaxe valide (démon Docker arrêté : image non tirée, rien lancé).

## À mettre à jour quand d'autres lots fusionnent
- **Lot G (images invités)** : section « Limites connues » de SHARING (images/icônes/formules/collage désactivés pour les invités).
- **Lot P** : affichage des gestes en cours et « X écrit… » (non mentionné dans la doc).
- **Lot R** : encadré « à compléter par le lot R » dans RELAY.md/fr (réglage de la liste de relais).
- **Navigateur (F)** : « pas de client navigateur » dans SHARING et RELAY.

## Non vérifié (écrit comme tel dans les docs)
Mise en service réelle (VPS, domaine, Let's Encrypt), journaux du relais (adresses IP ?), compatibilité d'Inkboard avec `access`, usage de la découverte d'adresse QUIC (UDP 7842), enregistrement du schéma `inkboard://` par les installeurs, piste « ouvrir l'adresse du relais dans le navigateur », LAN sans Internet sur deux machines.
