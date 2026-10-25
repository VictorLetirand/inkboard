# Lot SF1 — images piégées et URL de relais : décisions

Commandes `cargo +1.99.1`. Tests : `-p inkboard-formats raster`, `-p inkboard-net`, `-p inkboard-app -- tests_session_images tests_session_host tests_session_guest`.

## Corrigé
- **S-01** : `raster.rs`, macro `sandboxed_options!` (résolveur `resolve_string` toujours vide, `data:` gardé) pour PNG, raster, texture et PDF.
  Un `<image href>` vers fichier local, UNC ou URL reste vide. Tests : pixel d'un PNG temporaire absent.
- **S-02** : `check_untrusted_svg` (liste blanche de balises, pas de `filter`/`mask`/`pattern`/`script`/`foreignObject`/`a`/animations/DOCTYPE,
  `href` interne ou `data:` bitmap, `url()` interne, `style` sans filter/mask/@import, bornes : 20 000 éléments, profondeur 48, 200 `use`,
  bitmaps embarqués bornés). Appliqué par l'hôte à la fin d'un envoi (`check_guest_upload`) et en pré-contrôle côté invité (message dédié).
  Les SVG de l'hôte lui-même ne sont PAS filtrés (seul S-01 s'applique).
- **S-03** : bornes d'en-tête 16 384 de côté / 40 Mpx (avant décodage ; `declared_dimensions` en repli si l'en-tête est illisible) ;
  `render.rs` : `decode_asset_within(max_texture_side)` réduit (20 000x1 -> 8192x1), refuse > 100 Mpx déclarés, texture <= 40 Mpx ;
  `svg_to_raster_max_side` borne aussi les SVG.
- **S-06** : `check_relay_url` (https, pas d'identifiants, port 443 ou >= 1024, IP publique, pas de nom local/mono-étiquette) ;
  `connect_ticket` retire les relais refusés (sauf liste configurée ou `allow_private_relays`, vrai pour `NetConfig::loopback()`) ;
  `diagnose_relay` : relais inconnu = https public seulement, chemin `/`, résolution puis connexion à l'adresse vérifiée, pas de repli
  sans vérification TLS, jamais de redirection, en-tête `Server` nettoyé. `diagnose_relay_with(DiagnoseOptions)`.
- **S-07** : `judge_relays` (join_ui) : refusé -> message d'erreur ; inconnu -> fenêtre Annuler/Continuer avant toute connexion ;
  `NetIssue.unknown_relay` : plus aucun conseil d'ouvrir/autoriser le domaine, avertissement à la place.

## Reste / non vérifié
- Un nom DNS public qui résout vers une adresse privée n'est pas bloqué pour la connexion iroh elle-même (seulement pour `diagnose_relay`).
- Chemin TLS non fiable de `diagnose_relay` (inconnu : arrêt) non testé (pas de serveur TLS de test) ; aucun essai réseau réel.
- Un invité reçoit sans filtre les SVG de l'hôte (S-09 non traité) ; rendu d'un SVG de l'hôte avec filtre lourd non borné en temps.
- `JoinUi.trusted_relays` / `GuestConfig.trusted_relays` : à alimenter par le réglage de relais (lot R) ; vide aujourd'hui.
