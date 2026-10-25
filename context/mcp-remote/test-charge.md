# Test de charge du partage en direct (lot T2)

*Branche `test/load`, base `a7b9160`. Objectif de conception (PLAN_REMOTE §0, D2) : 20 participants confortables, plafond par défaut 10, charge validée à 50–100 sans casse.*



## 1. Méthode

### 1.1 Ce qui est mesuré, et avec quoi

| Élément | Où | Comment |
|---|---|---|
| **Hôte** | test ignoré `tests_crowd::crowd_host` (`inkboard-app`) | le VRAI `SessionHost` + le VRAI transport iroh (`IrohTransport::host`), un `Editor` sur le tableau seed, boucle à 62 tours/s comme l'application (un `poll` par image) ; accepte toute demande ; une « personne hôte » déplace/ajoute/supprime des objets toutes les 1,5–5,5 s et envoie son curseur à 20 Hz. Mode `app` (`CROWD_APP=1`) : l'`App` complète d'egui_kittest au lieu du seul `SessionHost`. |
| **Invités** | binaire `crowd guests` (crate `inkboard-crowd`), **autre processus** (voire autre machine) | le VRAI `inkboard_session::Client` + canal chiffré + un `IrohTransport::guest` par invité (identité propre, gardée pendant tout son séjour, donc reprise de place possible). Plusieurs processus se partagent la numérotation (`--first-index`). |
| **Tableaux seed** | `inkboard_crowd::board::seed_document` | 1 000 / 5 000 / 20 000 objets mélangés (formes, post-it, textes riches, blocs de code, formules, icônes SVG, connecteurs liés, tableaux, tracés de 40–400 points, 50 photos PNG de 20 Ko à 6 Mo), 2 / 4 / 6 pages ; graine reproductible. |
| **Profil des invités** | `inkboard_crowd::profile::Profile` | curseur à 30 Hz en rafales (60 % du temps actif), tracés (flux de points toutes les 50 ms), créations (formes, textes riches, connecteurs, icônes), déplacements (flux d'états transitoires à 20 Hz), suppressions, éditions de texte, remises à l'avant (liste complète d'identifiants), annulations, collage d'images (30–400 Ko ou 2–8 Mo, par morceaux avec accusés), 20 % de lecture seule, 10 % d'invités sur lien lent (200 ms ± 80 ms de latence par sens, 2 % de perte en entrée, 512 kbit/s de débit descendant), arrivées étalées, départs/retours, coupures et reprises de place. Tout est paramétrable (`--cursor-hz`, `--slow`, `--drops-per-hour`, `--mean-stay`, `--stuck`…) et reproductible (`--seed`). |

### 1.2 Mesures produites

Une exécution écrit dans son dossier : `host.json` + `host-timeline.csv` (une ligne par seconde : durée des tours, CPU, RSS, débits, files, membres, journal), `guests-<processus>.json` + `-timeline.csv`, `summary.md` (tableau + convergence). `crowd matrix DOSSIER…` assemble une ligne par exécution.

- **Arrivée d'un invité** (ms, depuis l'envoi du `Hello`) : `Welcome`, instantané complet (`SnapshotLoaded`), images (toutes les images du tableau reçues et vérifiées par hachage, 4 à la fois comme le vrai client) ; poids de l'instantané reçu.
- **Latence modification → visible chez les autres** : les objets créés portent dans leur identifiant l'heure d'envoi (µs, horloge murale) ; les autres invités **de la même machine** la relisent à la réception (même horloge). Même principe pour le curseur (champ `tool`). **Aller-retour d'une modification** : de `edit` à l'`Applied` qui la confirme (indépendant des horloges, donc valable pour une autre machine). Médiane, p95, p99 ; les invités sur lien lent sont comptés à part.
- **Hôte** : durée de chaque tour de boucle (`poll` + ce que l'hôte fait autour), intervalle entre deux tours, CPU et RSS du processus (crate `sysinfo`), charge de toute la machine, octets émis/reçus (enveloppe de connexion qui compte), taille de la file d'envoi de chaque connexion (`IrohConn::probe`), journal des opérations (`SessionHost::stats`).
- **Convergence** : empreinte SHA-256 du tableau (pages, objets dans l'ordre ; ni la vue ni les octets d'images) de l'hôte à la fin comparée à celle de chaque invité après une période de calme (`--settle`).
- **Coût d'une image de l'interface** : test ignoré `tests_crowd::crowd_ui_frames` : `App` complète (kittest, sans fenêtre ni GPU), avec N invités simulés en mémoire qui bougent leur curseur à 30 Hz et dessinent/déplacent devant l'hôte ; seul `step()` est chronométré.

### 1.3 Limites de la simulation

- Les invités sont des clients complets mais **sans interface** : ils ne dessinent rien (le coût de rendu côté invité n'est pas mesuré) et gardent seulement le nombre et les empreintes des images reçues.
- Un invité simulé ≠ une personne : les taux d'actions sont des hypothèses (tableau ci-dessus) ; un atelier où tout le monde dessine en continu serait plus chargé.
- Sur une seule machine, l'hôte et les invités se partagent les cœurs même en deux processus ; les chiffres « 100 invités en boucle locale » sont donc **pessimistes pour l'hôte** (moins de CPU disponible) et **optimistes pour le réseau** (pas de latence, pas de perte). La charge de la machine (colonne « machine CPU % ») est relevée à chaque exécution ; d'autres agents compilaient en parallèle sur ce poste pendant les mesures.
- La perte et la latence « lien lent » sont injectées **côté invité** (après la lecture QUIC) : le transport lui-même reste fiable, donc c'est la pression sur l'hôte (file qui grossit) et la reprise par `Resync` qui sont réalistes, pas le comportement du contrôle de congestion QUIC.
- Les latences « visible chez les autres » ne sont calculées qu'entre invités d'une même machine.
- Relais public : un seul essai court, avec un petit tableau (voir la section dédiée).

## 2. Rejouer

```text
# 1. construire (profil release sans LTO : même code chaud, compilation bien plus courte)
export CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo +1.99.1 build --release -p inkboard-crowd
cargo +1.99.1 test --release -p inkboard-app --no-run        # affiche le chemin de l'exécutable de test

# 2. une exécution : hôte + 50 invités sur un tableau de 5 000 objets, 40 s d'activité puis 12 s de calme
target/release/crowd run --host-exe target/release/deps/inkboard-XXXX.exe --guests 50 --objects 5000 --procs 2 \
    --duration 40 --settle 12 --out out/mon-essai --wait-idle 45
# options du profil : --profile default|calm|busy --cursor-hz 30 --slow 0.1 --drops-per-hour 12 --mean-stay 600 --stuck 0.05 --no-images ...

# 3. la matrice complète (5/20/50/100 invités × 1 000/5 000/20 000 objets), tableau dans OUT/matrix.md
bash scripts/crowd-matrix.sh out/ma-matrice "5 20 50 100" "1000 5000 20000"

# 4. le coût d'une image de l'application avec N participants (sans fenêtre)
CROWD_OBJECTS=5000 CROWD_PARTICIPANTS=0,5,10,20,50 cargo +1.99.1 test --release -p inkboard-app crowd_ui_frames -- --ignored --nocapture

# 5. invités sur une autre machine (Linux, Docker, CPU/mémoire limités) : voir scripts/crowd-remote.sh
```

Taille d'un tableau seed : `crowd board --objects 20000`.

## 3. Résultats (2026-10-25, hôte réel + invités simulés sur la même machine, boucle locale, 24 cœurs)

Tableaux complets : `charge/matrice-profil-realiste.md` (profil par défaut : 10 % d'invités lents, 5 % figés, coupures/reprises) et `charge/matrice-controle-sans-lents.md` (contrôle : `--slow 0 --stuck 0`). 40 s d'activité puis 12 s de calme par exécution.

| Invités | Objets | Arrivée d'un invité (snapshot) médiane / p95 | Confirmation d'une modification médiane / p95 | Tour de boucle de l'hôte, max | CPU hôte médian | Exclusions |
|---|---|---|---|---|---|---|
| 5 | 20 000 | 0,24 s / 1,0 s | 12 / 42 ms | 66 ms | 0,09 cœur | 0 |
| 20 | 20 000 | 0,5 s / 2,0 s | 14 / 314 ms | 80 ms | 0,27 cœur | 0 |
| 50 | 20 000 | 1,2 s / 3,8 s | 17 / 1 203 ms | 88 ms | 0,63 cœur | 0 |
| 100 | 20 000 | 1,6 s / 6,1 s | 111 / 4 189 ms | 155 ms | 2,1 cœurs | 0 |
| 100 | 5 000 | 0,7 s / 2,7 s | 123 / 1 342 ms | 33 ms | 1,6 cœur | 0 |

- **Avant les correctifs du lot** (même scénario, 100 invités × 20 000 objets) : tours de l'hôte jusqu'à 31 s, 35 exclusions, divergences. **Après** : plus aucun gel ni exclusion, aucune divergence.
- **Convergence** : dans le profil réaliste, 19/20, 48/50, 90/100 invités ont le même tableau que l'hôte à la fin. Les non-convergés sont toujours les mêmes (G9, G26, G76…) : les invités lents (512 kbit/s, 200 ms) ou figés du profil ; à 512 kbit/s un instantané de 4 Mio prend plus d'une minute, donc ils reçoivent encore à la fin des 52 s. **Contrôle sans invités dégradés : 50/50 et 100/100 convergent** (5 000 et 20 000 objets), aucun invité divergent.
- **Mémoire de l'hôte** : 140–245 Mio (≤ 20 invités), 250–350 Mio (50), 660–1 040 Mio (100).
- **Conclusion pour la conception** : 20 participants sont confortables même sur un gros tableau (confirmation < 20 ms médiane, hôte < 0,3 cœur) ; 50 sont corrects ; 100 fonctionnent sans casse mais avec des pics de latence de plusieurs secondes et un hôte à 2–3 cœurs : à ne pas promettre comme usage normal. Le plafond par défaut de 10 reste très prudent.

### Limites et points non expliqués
- **Latence « visible chez les autres », p99** : 6 à 20 s dans plusieurs exécutions, y compris le contrôle. Hypothèse (non vérifiée) : invités qui reprennent leur place après une coupure simulée et rattrapent leur retard ; la médiane (11–14 ms jusqu'à 50 invités) n'est pas touchée. À investiguer si des utilisateurs signalent des mises à jour tardives.
- Pas d'interface côté invités simulés (coût de rendu non mesuré) ; boucle locale (pas de vrai réseau, optimiste) sur une machine partagée entre hôte et invités (pessimiste pour le CPU de l'hôte).
- **Non fait faute de temps/limite d'usage** : session d'endurance de 60–90 min (fuites mémoire), essai avec des invités sur le serveur Linux (vraie seconde machine), essai de 10–20 invités par le relais public, mesure du coût d'une image de l'interface avec N participants (`crowd_ui_frames`, outil prêt).
