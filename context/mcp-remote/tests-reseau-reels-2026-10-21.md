# Essais réseau réels du transport (prototype S1, `iroh` 1.4.0) — 2026-10-21

Prototype jetable de `spikes/s1-iroh/` (échange de petits messages et de 1 Mio). **Ce n'est pas le protocole de session** (lot B) : ces essais mesurent seulement le transport. Hôte : PC Windows de maison (relais public de n0 `euc1-1`, mode relais seul).

## Matériel et réseaux
| Appareil | Système | Réseau | Adresse publique |
|---|---|---|---|
| PC (hôte) | Windows 11 | box maison | 203.0.113.10 |
| Serveur « homelab » | Linux x86_64 (via SSH/Tailscale, build Docker `rust:1.99`) | **même box** | 203.0.113.10 (même réseau) |
| Téléphone OnePlus | Android, Chrome | **5G opérateur** (puis Wi-Fi pour un essai) | 198.51.100.7 (autre réseau) |

Le homelab n'est donc **pas** un autre réseau (même box) ; seul le téléphone en 5G est un vrai autre réseau.

## Mesures (1 000 aller-retours ; 200 pour les invités du homelab)
| Essai | Médiane | p95 | Débit | Chemin |
|---|---|---|---|---|
| Direct local (Windows) | 0,07 ms | 0,12 ms | ~2 Gbit/s | ip |
| Natif Windows ↔ relais n0 | 49 ms | 50 ms | 5–16 Mbit/s | relais |
| Invité Linux (homelab) → hôte Windows | 49,2 ms | 50,2 ms | 15 ↓ / 20 ↑ Mbit/s | relais |
| Invité Windows → hôte Linux (homelab) | 49,9 ms | 50,8 ms | 20 ↓ / 16 ↑ Mbit/s | relais |
| Navigateur intégré de Claude (Chromium 152) | 49,6 ms | 51,2 ms | 17 ↓ / 15 ↑ Mbit/s | relais |
| **Téléphone Android, Chrome, 5G** | 82–88 ms | 96–108 ms | 1,6–7 ↓ / 2,6–8,4 ↑ Mbit/s | relais |
| Téléphone, Wi-Fi | 59 ms | 120 ms | 12 ↓ / 19 ↑ Mbit/s | relais |

Une page servie en **http** sur une adresse non sécurisée (`isSecureContext = false`) suffit : l'invité rejoint.

## Coupures réseau (téléphone, session de 1 000 échanges)
| Scénario | Délai d'inactivité | Résultat | Pire blocage |
|---|---|---|---|
| Mode avion ~5 s | 30 s (défaut) | survit, 1 000 échanges | 9,96 s |
| Passage mobile ↔ Wi-Fi | 30 s | survit | 0,86 s |
| Mode avion 20 s | 30 s (défaut) | **perdue** : « connection lost: timed out » (l'hôte : `TimedOut` 28 s après le dernier message) | — |
| Mode avion 20 s | **120 s** | survit | 34,3 s |
| Mode avion 60 s | **120 s** | survit | 89,6 s |

Lecture : le défaut de 30 s (vérifié dans le code d'`iroh` 1.4.0 : `max_idle_timeout`, 30 s par défaut) fait perdre toute coupure de plus de ~15–20 s. Réglé à 120 s des deux côtés (`QuicTransportConfig::builder().max_idle_timeout(…)`), une coupure de 60 s est absorbée. **La reprise après le retour du réseau est lente et croît avec la durée de la coupure** : environ 5 s, 14 s puis 30 s de blocage supplémentaire pour des coupures de 5, 20 et 60 s (probable temporisation exponentielle des relances). Le produit devra : afficher « reconnexion… » après 2–3 s, mettre les modifications en attente, et tenter de se reconnecter vite (événement `online` du navigateur, nouveau sondage du chemin) au lieu d'attendre.

## Décisions et recommandations issues de ces essais
- Régler le **délai d'inactivité à 120 s** (hôte et invité) ; garder une reconnexion applicative avec le même identifiant de participant pour les coupures plus longues.
- Prévoir l'accélération de la reprise (le temps de reprise observé est le principal défaut d'usage).
- Débit sur 5G : 2,6–8 Mbit/s en montée : limiter la taille des images qu'un invité mobile envoie et transférer les images par morceaux.

## Non vérifié
Un seul opérateur, un seul relais public ; chemin direct entre deux réseaux ; réseau d'entreprise ; Firefox/Safari/iOS ; coupures de plus de 60 s ; relais auto-hébergé ; le vrai protocole de session au-dessus d'`iroh`. Le homelab (même box) ne teste pas le perçage de NAT.
