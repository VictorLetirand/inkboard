# Lot R — reprise rapide, démarrage sans gel, relais personnalisés (commandes `cargo +1.99.1`)

## Ce que permet iroh 1.4
`Endpoint::network_change()` existe : l'endpoint relit l'état des interfaces ; si le réseau a changé, iroh rebind ses sockets, relance
la sonde de chemins (ping de chaque chemin) et le trou de NAT. iroh surveille déjà seul le réseau (natif) ; l'appel sert de « coup de coude ».
Exposé par `IrohTransport::network_change()` (non bloquant, hôte et invité). Pas de recréation du lien : le même point de terminaison
(donc la même identité) est gardé, c'est lui qui permet à l'hôte de rendre la place sans nouvelle demande.

## Reprise rapide
- `NetWatch` (session_guest) + `LocalAddrWatch` (session_net) : adresse locale choisie par le système pour sortir (UDP `connect`, rien n'est envoyé,
  aucune dépendance), regardée 1 fois/s ; déclenche au retour d'une route ou à un changement d'adresse. Hôte : `refresh` appelle `network_change`.
- `GuestSession::retry_now()` (bouton « Réessayer maintenant » et signal réseau) : `GuestLink::network_changed`, coupe la tentative en cours
  (`Dialing::close` -> `CancelToken` -> `connect_ticket_cancellable`), nouvelle tentative aussitôt, même identifiant.
- Tentatives : 4 rapprochées (1 s) puis 2, 4, 8 s (`fast_retries`, `retry_max_ms`) ; une tentative sans réponse est coupée après 6 s (`reconnect_attempt_ms`).
- Bandeau : « Reconnexion… (tentative N) » + bouton.

## Sans gel
- `SessionNet::start_job` (fil, annulable, « Démarrage… ») ; `HostTransport::release` et `Drop` de `NetLink` ferment sur un fil (`close_in_background`) ;
  `on_exit` attend au plus 1,5 s (`wait_background_closes`) pour laisser partir le dernier `Bye`.

## Relais
Fichier `relays` (`session_net::RELAYS_FILE`), une URL https par ligne, 4 max ; invalide = erreur (jamais de repli silencieux). Un choix explicite
(`Disabled`) n'est pas écrasé.

## Non vérifié / limites
- Aucune vraie coupure réseau essayée (mode avion, changement d'interface) : seulement temps virtuel + boucle locale.
- `LocalAddrWatch` ne voit pas : un réseau « up » sans Internet (portail captif), un retour à la même adresse en moins d'1 s, IPv6 seul sans route v4/v6 testée.
- Quitter : le `Bye` peut être perdu si un pair ne répond plus (plafond 1,5 s) ; la fenêtre peut rester visible ce temps-là (pas masquée).
- Pas d'événement `online` navigateur (lot web).
