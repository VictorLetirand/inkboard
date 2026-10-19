# Décisions de l'utilisateur — 2026-10-19

> Suite du plan de tests (`PLAN_TESTS_AUTO.md`, lots A à J faits). Ces décisions sont **notées, pas encore appliquées**.
> Priorité donnée : tout ce qui fiabilise l'application. Les workflows GitHub viennent plus tard.

## Décidé : à faire (fiabilisation)

| # | Sujet | Décision | Précisions |
|---|---|---|---|
| D1 | `quick-xml` (RUSTSEC-2026-0194/0195) | **Faire la montée** egui / eframe / accesskit | Objectif : `quick-xml >= 0.41` (via `zbus_xml` 6), puis retirer les deux `ignore` de `deny.toml`. Vérifier : tests, captures (`--screenshot`), `cargo deny check -W unmaintained`. Surveiller les régressions d'interface après la montée. |
| D2 | Ctrl+N vide le tableau sans confirmation et supprime l'autosave | **Au jugement de l'agent** (« si tu veux, pas de souci ») | Proposition : demander confirmation si le tableau contient des modifications non enregistrées, et ne pas supprimer l'autosave tant que l'utilisateur n'a pas confirmé. |
| D3 | Action sans raccourci quand toutes ses entrées de `keymap.json` sont illisibles | **Au jugement de l'agent** | Proposition : l'action garde son raccourci par défaut et l'erreur est signalée (cohérent avec BUG-9). Mettre à jour le test unitaire existant qui fixe l'ancien comportement. |
| D4 | Icônes intégrées draw.io (`image=img/lib/...`) ignorées à l'import avec leur libellé | **À corriger** | La bibliothèque de 28 800 icônes existe maintenant : faire correspondre `img/lib/...` à une icône de la bibliothèque ; à défaut, garder au moins un rectangle avec son libellé. Compter dans `report` ce qui reste non résolu. |
| D5 | Libellé « Diamond » coupé en « Diamon / d » dans un losange de 160×110 | **À corriger proprement** | Cause à chercher dans la zone de texte utile d'un losange (largeur disponible). Ne pas couper un mot au milieu quand une zone plus large est possible. |
| D6 | « Modifications… » à la place de « Unsaved » en français | **À corriger proprement** | Choisir un libellé français clair (ex. « Non enregistré ») et vérifier la traduction de toute la carte du titre. |
| D7 | `THIRD_PARTY_LICENSES.md` : manque `CC0-1.0` et la pile wgpu (56 crates) | **À faire** | Régénérer avec `scripts/gen_third_party.py` ; vérifier qu'il couvre les crates que les binaires publiés embarquent réellement ; ajouter un test ou un contrôle CI léger qui compare avec `cargo deny list`/`cargo metadata`. |
| D8 | Extension `.inkboard` | **Fait (voir Journal « D8 »)** | Remplacer `.inkboard.json` pour l'enregistrement ; continuer à ouvrir `.inkboard.json` et `.tableau.json` ; association de fichier dans l'installeur (NSIS/MSI, deb, dmg) ; mettre à jour documentation, tests, menus et sélecteurs de fichiers. |
| D9 | Verrou d'outil peu visible (cadenas / touche `Q`) | **À faire (second temps)** | Info-bulle claire, et double-clic sur un outil pour le verrouiller (comme PowerPoint) ; état verrouillé bien visible. |

## Décidé : autres consignes

- **Test de stylet** (lot J) : l'agent le lance lui-même (autorisation donnée). Résultat à consigner dans le Journal.
- **Workflows GitHub** (`smoke`, `bench`, `visual`, `release` vérifications, job `deny`) : **reportés**. Aucun push ni lancement pour l'instant.
- **BUG-8** (Ctrl+= zoome aussi l'interface) : confirmé seulement dans le harnais de test ; à vérifier dans une vraie fenêtre.

## Ordre proposé

1. D5, D6 (petits, locaux) · 2. D3, D2 (comportements) · 3. D4 (import draw.io) · 4. D7 (licences) · 5. D1 (montée, risque le plus large : à faire sur une branche à part, après que tout le reste est commité) · 6. D8 puis D9.

## État au 2026-10-19 (fin de journée)

| # | État | Commits |
|---|---|---|
| D2 | Fait : confirmation avant « Nouveau » si modifications non enregistrées | `9def7a8` |
| D3 | Fait : le raccourci par défaut est gardé si toutes les entrées sont illisibles | `63d9e4d` |
| D4 | Fait : icônes `img/lib` résolues (app) ou rectangle + libellé (CLI/MCP) | `5ae9d49`, `ee5fedc` |
| D5 | Fait : zone de texte du losange élargie | `9a771e7` |
| D6 | Fait : « Non enregistré » | `d843225` |
| D7 | Fait : licences régénérées + test de contrôle | `e76cd73`, `a4a09f8` |
| D8 | Fait (association `.msi` probablement absente, installeurs jamais lancés) | `09feeca`, `d1b006a`, `60d5eb9`, `7cf662a` |
| D9 | Fait (rendu du badge non vérifié à l'écran) | `b26a8ec` |
| D1 | Fait par `Cargo.lock` seul (`zbus_xml` 5.2.1, pas de montée egui), voir Journal « D1 » | branche `chore/upgrade-egui` |
