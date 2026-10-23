# Lot G — images pour les invités : décisions

Commandes `cargo +1.99.1`. Tests : `cargo +1.99.1 test -p inkboard-app images::` (12 tests, SimNet + horloge virtuelle, `tests_session_images.rs`,
inclus depuis `tests_session_guest.rs` par `#[path]` car `main.rs` n'est pas dans la liste des fichiers du lot).

## Protocole (additif, `inkboard-session`)
- `Message::AssetAck { hash, have, status }` (hôte -> invité), `UploadStatus { Receiving, Stored, Refused { reason } }` : un accusé par morceau reçu
  (`have` = octets tenus dans l'ordre). Sert à cadencer, reprendre et arrêter. `AssetAssembler::with_unknown_size` (image apprise d'une modification).
- Hôte (`session_host.rs`, `on_chunk`) : image déjà connue (tableau ou pool d'envois) = `Stored` sans quota ; un trou ou un doublon n'efface plus le tampon
  (on répond `have`) ; refus (taille, empreinte, type, dimensions, quota) = avertissement `Rejected{0}` PUIS `Refused`. Dimensions lues dans l'en-tête avant
  tout décodage (16 384 de côté, 80 Mpx) ; un en-tête illisible passe (le décodeur échouera de même, repère « illisible » à l'affichage).

## Invité (`session_guest.rs`)
- `submit_local(ops, assets)` : pour chaque image que l'hôte n'a pas (ni par id, ni par empreinte du manifeste), l'identifiant devient l'empreinte SHA-256
  (c'est ce que l'hôte exige), l'image est mise en file (une à la fois, morceaux de 128 Kio, fenêtre de 4 non accusés, 30 morceaux/s, renvoi après 2 s sans
  accusé). Le `Submit` est appliqué tout de suite en optimiste mais **retenu** (`held`, dans l'ordre) tant qu'un envoi est en cours.
- Pré-contrôles locaux (avant tout envoi) : > 15 Mio, type non image, dimensions hors bornes, quota estimé (100 Mio) -> message + `false` (le miroir est
  rafraîchi : modification annulée). Refus de l'hôte (`Refused`) : les `Submit` concernés sont annulés via un `Rejected` injecté dans le `Client`, un seul message.
- Reprise : à `Joined`, l'envoi repart de `acked` ; l'hôte garde le tampon de l'invité (place gardée 60 s) ; un hôte qui a tout oublié répond `have` plus petit.
- Réception : les `Applied` sont examinés ; un identifiant en forme d'empreinte absent du manifeste est demandé par empreinte (taille inconnue).
  Image reçue : type + dimensions vérifiés, sinon repère SVG « croix » (`unreadable_asset`) ; contenu ne correspondant pas à l'empreinte : 3 essais puis repère.
- Indicateur : `upload_progress()` -> « Envoi des images… N % » dans la barre de l'invité. Lecteurs : boutons grisés, `dropped_files` refusé avec message.

## Limites connues / à proposer
- Une image ajoutée par l'**hôte** après l'instantané a un identifiant aléatoire non annoncé : l'invité ne la reçoit qu'au prochain instantané (limite du lot D,
  inchangée). Correctif possible : message hôte -> invités annonçant `AssetInfo` à chaque nouvel actif.
- Un lot de plusieurs images dans un même geste est annulé en bloc si l'une est refusée localement.
- Pool d'envois de l'hôte : 32 images ; une image évincée avant son `Submit` donne « not uploaded » (l'utilisateur recommence).
- Non vérifié : transfert réel via iroh (SimNet seulement), aucun essai manuel, références visuelles non régénérées.
