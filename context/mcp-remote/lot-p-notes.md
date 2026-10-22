# Lot P — finitions de présence : décisions

Commandes `cargo +1.99.1`. Tests : `cargo +1.99.1 test -p inkboard-app presence_lot` (SimNet, horloge virtuelle, hôte + invités réels).
Captures : `cargo +1.99.1 test -p inkboard-app capture_lot_p -- --ignored` -> `lot-p-stroke.png`, `lot-p-badge.png`.

## Rendu
- `RemotePresence` (presence.rs) : `strokes` (tracés en cours, couleur de l'auteur), `ghosts` (objets déplacés/redimensionnés, hôte seulement :
  l'original est estompé, la version de l'invité est dessinée par-dessus, jamais écrite dans le tableau), `RemotePerson.editing/viewport`.
  Chez un invité, un déplacement distant reste affiché en modifiant le miroir (mécanisme du lot D, inchangé).
- Fin d'un geste : `settle(auteur)` à la réception de son `Submit` (hôte : `HostEvent::Settled`) ou de son `Applied` (invité), et à la coupure
  de sa connexion (hôte). Filet : `prune` après 3 s sans point ni signe de vie ; un `Presence` de l'auteur renouvelle (`touch`) :
  un geste tenu immobile n'est pas pris pour un geste abandonné.
- Badge « X écrit… » : cadre + étiquette sur l'objet de `Presence.editing` ; disparaît après 6 s sans nouvelle (présence répétée toutes les 2 s,
  hôte compris : `PRESENCE_KEEPALIVE`). Informatif, ne bloque rien. L'hôte annonce aussi son propre texte en cours d'édition.
- `glide_to` : une position répétée par le keep-alive n'est plus une activité (le curseur au repos s'efface après 5 s comme prévu).

## Envoi (invité)
- `Presence` : <= 20/s (+ keep-alive 2 s), avec `viewport`. `StrokePoints` : 1 message par tick de 50 ms. `Transient` : **1 objet par tick**
  (rotation entre les objets d'un geste de groupe) : curseur + un éphémère = 40 msg/s, sous la limite hôte de 60/s. Conséquence : dans un
  déplacement de groupe, les objets sont rafraîchis à tour de rôle chez les autres (le résultat validé est exact).
- Mesure : `GuestSession.sent_log` (cfg(test)), temps de la session (virtuel).

## Suivre
- État unique `agent.remote.following` (hôte et invité) ; `presence::follow_frame` (avant le canvas) glisse la vue vers le `viewport`
  (page, zoom, centre) du participant, repli sur son curseur. Fin : clic dans le canvas (tools.rs), vue/page modifiée autrement
  (molette, raccourci), touche hors champ de saisie, début d'édition de texte, départ du participant, bouton « Ne plus suivre ».
- Invité : liste des autres participants avec « Suivre » (`presence::follow_ui`, sous le bandeau) ; hôte : liste de la fenêtre Partager.

## Agent MCP chez les invités
- Défaut trouvé et corrigé : les cibles de l'événement `Agent` étaient cherchées dans le miroir, rafraîchi seulement après les messages
  de la même image ; un objet qu'on vient de créer n'y était pas (pas de halo, pas de déplacement). Elles sont cherchées dans le tableau de la session.

## Hooks de test (additifs)
`RemotePresence.skew` (le temps de présence avance sans attendre), `Share.test_clock` (horloge de l'hôte), `GuestSession.sent_log`.

## Non fait / non vérifié
- Les gestes en cours de l'hôte lui-même (tracé, déplacement) ne sont pas envoyés aux invités (hors demande ; proposition : même envoi côté hôte).
- Aucun essai réseau réel ni captures en vraie fenêtre ; pas de lissage du tracé distant (segments droits entre points reçus à 20 Hz).
