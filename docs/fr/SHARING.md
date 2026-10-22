# Partage en direct : partager un tableau et rejoindre une session

*English version: [../SHARING.md](../SHARING.md)*

Inkboard peut partager un tableau **en direct** : vous cliquez sur **Partager**, vous obtenez un lien, et les personnes qui l'ouvrent (après votre accord) voient et modifient le même tableau en même temps, avec le curseur de chacun. Vous êtes le « serveur » : le tableau reste sur votre ordinateur et la session s'arrête quand vous la fermez.

> **État.** Le partage en direct est en cours de développement (versions de préparation). Cette page décrit ce que fait le code aujourd'hui ; ce qui n'est pas encore fait est écrit comme tel. L'hébergement de votre propre service de connexion est décrit dans [RELAY.md](RELAY.md).

## En bref

- **Rien ne se connecte tant que vous n'avez pas cliqué sur « Démarrer le partage » (hôte) ou « Rejoindre » (invité).** Sinon, Inkboard reste 100 % hors ligne.
- Le lien est un **secret** : ne le donnez qu'aux personnes que vous voulez inviter.
- Chaque nouvel arrivant a besoin de **votre accord**. Vous pouvez exclure quelqu'un à tout moment.
- Tout est **chiffré de bout en bout** ; le service de connexion (le « relais ») ne transporte que des données chiffrées.
- Sur un réseau restreint (entreprise, école), voir [Réseaux restreints](#réseaux-restreints-entreprise-école).

---

## Partager un tableau (hôte)

1. Ouvrez le tableau à partager.
2. Cliquez sur **Partager** dans la barre de titre. La fenêtre « Partager » s'ouvre et indique « Non partagé ».
3. Réglages facultatifs, avant ou pendant la session :
   - **Votre nom** : le nom affiché sur votre curseur et dans la liste des participants (par défaut « Hôte » ; Inkboard n'utilise jamais le nom de votre session Windows). Il est mémorisé.
   - **Participants (maximum)** : le nombre maximum de personnes dans la session, **vous compris**. Par défaut 10 ; réglable de 2 à 64. Inkboard est conçu et testé pour environ 20 personnes.
   - **Accepter automatiquement en lecture les personnes qui ont le lien de lecture** : désactivé par défaut. Activé, toute personne qui ouvre le lien de lecture entre sans fenêtre d'accord (elle peut seulement regarder).
4. Cliquez sur **Démarrer le partage**. Inkboard contacte alors le service de connexion, ce qui peut prendre quelques secondes.
5. Deux liens apparaissent, chacun avec un bouton **Copier** : le **Lien d'édition** et le **Lien de lecture**. Collez celui que vous voulez dans un message, un courriel, une discussion.

Ensuite, un point vert et un nombre (« ● 3 ») dans la barre de titre indiquent que la session est active et combien de personnes y sont (vous compris). Survolez-le pour les détails.

### Lien d'édition ou lien de lecture ?

| | Lien d'édition | Lien de lecture |
|---|---|---|
| Ce que la personne peut faire | Modifier le tableau (si vous l'acceptez en édition) | Regarder, se déplacer, pointer avec le laser ; rien modifier |
| À qui le donner | Les personnes à qui vous faites confiance pour modifier | Public, relecteurs |

- **Ne partagez jamais le lien d'édition publiquement** (salon public, réseau social, page web). Quiconque l'a peut demander à entrer avec le droit d'édition. Vous devez quand même accepter chaque personne, mais une avalanche de demandes est désagréable, et un lien qui circule est un lien que vous ne maîtrisez plus.
- Le lien est un secret : il contient la clé qui chiffre la session et des indications de connexion vers votre ordinateur. Traitez-le comme un mot de passe.
- Le droit réellement accordé est le plus faible de celui que permet le lien et de celui que vous choisissez : une personne arrivée avec le lien de lecture ne peut pas être acceptée en édition (« Accepter en édition » est grisé).
- Les droits sont appliqués par votre ordinateur, pas seulement cachés dans l'interface : les modifications d'un invité en lecture sont refusées.

### Accepter, refuser, exclure

Quand quelqu'un ouvre un lien, une fenêtre **Demandes pour rejoindre** affiche « *Anna* veut rejoindre » (la fenêtre Partager s'ouvre aussi, et une notification apparaît) avec trois boutons :

- **Accepter en édition** (seulement pour les personnes venues avec le lien d'édition),
- **Accepter en lecture**,
- **Refuser**.

Le nom est celui que l'invité a saisi. C'est une simple étiquette : ce n'est pas une preuve d'identité. Si deux personnes choisissent le même nom, Inkboard affiche « Anna » et « Anna (2) ».

Dans la fenêtre Partager, la liste **Participants** montre chaque personne avec un point de couleur. À côté de chaque invité :

- **Renommer** : change le nom affiché (utile si quelqu'un a choisi un nom trompeur).
- Le bouton de droit (**Peut modifier** / **Lecture seule**) : cliquez pour passer la personne à l'autre droit, à tout moment. L'invité est prévenu (« L'hôte a passé votre accès en lecture seule », « L'hôte vous autorise maintenant à modifier »).
- **Suivre** : votre vue suit cette personne (même page, même zone visible). Cliquez sur **Ne plus suivre** pour retrouver votre vue.
- **Exclure** : retire la personne de la session. Elle ne peut pas revenir avec le même lien depuis la même installation.

Un invité qui perd la connexion apparaît avec « (absent) » et garde sa place 60 secondes.

### Si un lien a fuité : nouveaux liens

Cliquez sur **Nouveaux liens (les anciens cessent de fonctionner)**. Les deux liens sont remplacés ; les anciens ne fonctionnent plus. Les personnes déjà présentes restent : excluez celles dont vous ne voulez pas.

### Arrêter le partage

Cliquez sur **Arrêter le partage** dans la fenêtre Partager (ou quittez Inkboard). Tout le monde est prévenu que l'hôte a mis fin à la session, et toutes les connexions sont fermées. Votre tableau reste le vôtre ; les invités n'ont rien écrit dans vos fichiers. Inkboard n'enregistre pas le tableau à votre place : enregistrez-le comme d'habitude.

---

## Rejoindre une session (invité)

1. Obtenez un lien auprès de l'hôte. Il ressemble à `inkboard://join#…` (ou `inkboard://join/…`).
2. Au choix :
   - **cliquez sur le lien** : si Inkboard est installé, il ouvre la fenêtre **Rejoindre une session** avec le lien déjà rempli (« Lien reçu ») ; ou
   - dans Inkboard, ouvrez le menu principal et choisissez **Rejoindre une session…**, puis collez le lien dans **Lien d'invitation**.
   Si le clic sur le lien ne fait rien (par exemple parce que l'installeur n'a pas enregistré les liens `inkboard://` sur votre système, ce qui n'a pas été vérifié sur toutes les plateformes), utilisez l'entrée du menu.
3. Saisissez **Votre nom** (il est pré-rempli avec le dernier nom utilisé).
4. Cliquez sur **Rejoindre**. La fenêtre affiche « Session de *nom de l'hôte* » et « En attente de l'accord de l'hôte… ». **Annuler** abandonne.
5. Quand l'hôte accepte, son tableau remplace le vôtre à l'écran, et un bandeau bleu en haut à droite affiche « Session de *nom de l'hôte* » (et « · Lecture seule » si c'est votre droit) avec un bouton **Quitter la session**.

Votre propre tableau est **mis de côté, pas perdu** : en quittant la session, il revient exactement comme il était. Pendant la session, « Nouveau tableau » et « Ouvrir » sont indisponibles, et rien du tableau de l'hôte n'est enregistré automatiquement sur votre disque.

### Travailler ensemble

- Vous voyez les **curseurs** des autres avec leur nom, et les objets qu'ils ont sélectionnés dans leur couleur.
- **Les modifications sont en direct.** Si deux personnes modifient le *même objet* au même moment, c'est la modification qui arrive en dernier chez l'hôte qui l'emporte pour cet objet. Il n'y a pas de co-édition caractère par caractère d'un même texte.
- **Annuler / rétablir en groupe : on ne défait que ses propres actions.** Ctrl+Z annule *votre* dernière modification, jamais celle de quelqu'un d'autre, et le Ctrl+Z de l'hôte n'annule que les modifications de l'hôte. Si quelqu'un d'autre a modifié un objet depuis votre action, cet objet est laissé tel quel (il n'est pas annulé) et une notification peut s'afficher. Rétablir fonctionne de la même façon.
- Le **laser** fonctionne pour tout le monde, invités en lecture seule compris.
- Si la connexion se coupe, un bandeau orange affiche **Reconnexion…**. Vos modifications sont conservées et envoyées au retour de la connexion ; le tableau reste affiché tel qu'il était. Une coupure courte (de quelques secondes à une minute) est absorbée ; après environ 2 minutes sans aucune réponse, la session s'arrête avec « La connexion a été perdue trop longtemps. »
- Si l'hôte ferme la session, un bandeau rouge l'indique, avec **Enregistrer une copie du tableau** (choisissez où enregistrer une copie du tableau partagé, en fichier `.inkboard` ordinaire) et **Quitter**. Vous pouvez aussi utiliser **Enregistrer** du menu principal à tout moment pour enregistrer une copie ; cela n'écrase jamais votre propre fichier.

### Quitter

Utilisez **Quitter la session** (bandeau bleu ou menu principal). Votre propre tableau revient.

---

## Le curseur de l'agent MCP

Si vous utilisez Inkboard avec un agent d'IA via MCP (voir [MCP.md](MCP.md)), l'agent apparaît comme un participant portant le nom du client (par défaut « Claude ») : un curseur coloré glisse vers les objets qu'il crée ou modifie, avec une courte traînée et un halo sur les objets touchés. Un simple regard sur le tableau (`render`, `describe_board`) est une pulsation discrète.

- **Activé par défaut** ; le réglage est **Animer l'agent** dans le menu principal (il est mémorisé). Il ne change que l'affichage, jamais le tableau, l'historique d'annulation ni ce que l'agent reçoit.
- **En session**, les actions de l'agent de l'hôte sont aussi montrées aux invités (chaque invité peut désactiver l'animation pour lui-même).
- L'agent fait partie de l'application de **l'hôte** : il agit avec les droits de l'hôte, ses modifications sont attribuées à l'hôte (donc le Ctrl+Z de l'hôte les annule), et il ne peut ni démarrer, ni arrêter, ni configurer une session de partage (aucun outil MCP ne le permet).
- Un invité ne peut pas piloter le tableau partagé avec son propre agent : le serveur MCP n'est pas actif pendant que vous êtes invité.

---

## Limites connues

- **Nombre de personnes** : 10 par défaut (vous compris), réglable. Conçu pour environ 20 ; les tests du protocole simulent 50 et 100 invités, mais aucune vraie session de cette taille n'a été essayée.
- **Images et icônes pour les invités** : dans la version actuelle, un invité **ne peut pas insérer** d'images, d'icônes, de formules, de diagrammes Mermaid, ni coller ou déposer d'images (les boutons sont grisés, ou un message indique « Pas encore disponible dans une session partagée »). Les invités voient bien les images de l'hôte. L'envoi d'images par un invité est prévu.
- **Pas encore de client navigateur** : pour rejoindre, il faut avoir Inkboard installé. Rejoindre depuis une page web sans rien installer est prévu, pas disponible.
- **Un tableau à la fois** : on est soit hôte, soit invité, pas les deux. Rejoindre une session arrête votre propre partage.
- **L'hôte doit rester en ligne.** Si l'hôte quitte ou perd le réseau plus d'une minute environ, la session s'arrête (les invités peuvent enregistrer une copie). Rien n'est hébergé pour vous.
- **Polices** : les polices disponibles sur chaque ordinateur servent à la mise en page. Un texte peut donc passer à la ligne différemment entre deux ordinateurs qui n'ont pas les mêmes polices. Un jeu de polices commun embarqué est prévu.
- **Démarrer et arrêter** peuvent figer la fenêtre jusqu'à 3 secondes, le temps de joindre le service de connexion.
- **La reprise après une longue coupure** peut être lente : après une coupure d'une minute, le tableau peut mettre une demi-minute de plus à revenir une fois le réseau rétabli.
- Essais réalisés jusqu'ici : réseau domestique, réseau mobile (5G) et réseau d'entreprise (où cela a échoué, voir plus bas). Les autres réseaux, types de NAT et navigateurs ne sont pas vérifiés.

---

## Confidentialité et sécurité

- **Hors ligne par défaut.** Inkboard ne se connecte à rien tant que l'hôte n'a pas cliqué sur **Démarrer le partage** ou qu'un invité n'a pas cliqué sur **Rejoindre**. (Quand une connexion échoue, Inkboard peut ensuite contacter l'adresse web du service de connexion pour en diagnostiquer la cause.)
- **Chiffrement de bout en bout.** La session est chiffrée par la connexion elle-même **et** par une clé qui n'est que dans le lien. Le relais, et toute personne qui écoute le réseau, voient des octets chiffrés, pas votre tableau.
- **Ce que le relais voit** : des métadonnées réseau, en particulier les **adresses IP**, les heures de connexion et le volume de données. C'est inhérent à tout relais. Par défaut, Inkboard utilise les relais publics gratuits du projet iroh (n0), prévus pour le développement et les loisirs et limités en débit. Pour un usage sérieux, vous pouvez [héberger votre propre relais](RELAY.md).
- **Connexions directes.** Quand deux ordinateurs peuvent se joindre directement (même réseau, VPN, ou traversée de NAT réussie), les données ne passent pas par le relais, et chaque côté peut voir l'adresse IP de l'autre. Le lien porte aussi des indications de connexion vers l'ordinateur de l'hôte (adresses), une raison de plus de ne pas le publier.
- **Vous décidez qui entre** : chaque arrivant a besoin de votre accord, les droits sont appliqués sur votre ordinateur, et vous pouvez exclure n'importe qui. Le nom est une étiquette, pas une identité.
- **Les invités n'ont aucun accès à vos fichiers.** Rien venant d'un invité ne peut lire ou écrire des fichiers sur votre ordinateur. Les images envoyées par les invités sont limitées en taille et ne sont pas décodées avant l'affichage. Un invité qui dépasse les limites (taille des messages, vitesse, quota) est averti, puis exclu.
- **Pas de compte, pas d'historique sur un serveur, pas de suivi.** Inkboard n'a pas de serveur à lui pour les sessions.
- Les modifications d'un invité s'appliquent au tableau de l'hôte ; pour garder le résultat, enregistrez-le. Pour conserver votre original, enregistrez une copie **avant** de partager.

## Réseaux restreints (entreprise, école)

Beaucoup de réseaux d'entreprise et d'école bloquent les services Internet inconnus. Si vous ne pouvez pas rejoindre (ou démarrer une session que d'autres peuvent rejoindre), Inkboard affiche une fenêtre intitulée **Impossible de rejoindre la session**.

### Ce que signifie le message

- *« Impossible de joindre le service de connexion. Un pare-feu ou une passerelle de sécurité (souvent en entreprise ou à l'école) le bloque probablement. »* Le « service de connexion » est le relais, le serveur qui permet à deux ordinateurs de se trouver. Votre réseau ne laisse pas Inkboard l'atteindre. Si Inkboard reconnaît la passerelle (par exemple Zscaler), le message la nomme.
- *« Pas de connexion : cet ordinateur ne semble pas relié à Internet. »* Vérifiez votre Wi-Fi ou votre câble.
- *« Le service de connexion répond, mais l'hôte est injoignable. »* Votre réseau est correct ; l'hôte a fermé la session ou n'est plus en ligne. Demandez-lui de vérifier.

### Les trois pistes

Quand le réseau est probablement en cause, la fenêtre propose trois pistes :

1. **Essayez un autre réseau**, par exemple le partage de connexion de votre téléphone.
2. **Ouvrez l'adresse du service dans votre navigateur** (la fenêtre affiche `https://<domaine>`) et acceptez la page d'avertissement s'il y en a une, puis réessayez. Certaines passerelles affichent une page d'avertissement (« catégorie inconnue ») avec un bouton « Continuer » ; un navigateur peut cliquer dessus, Inkboard non. *Cette piste n'est pas vérifiée : lors de notre essai, nous avons vu la page d'avertissement, mais nous n'avons pas confirmé qu'accepter l'avertissement débloque Inkboard.*
3. **Demandez au service informatique d'autoriser le domaine** indiqué dans la fenêtre.

### Le bouton « Copier le diagnostic »

Le bouton **Copier le diagnostic** place dans le presse-papiers un court texte (en anglais, pour que votre service informatique puisse le lire). Il contient : la version d'Inkboard, le domaine et l'adresse du service de connexion, s'il était joignable, le code d'état HTTP, l'en-tête `Server`, la passerelle de sécurité si elle a été reconnue, et l'erreur. Il ne contient **rien sur votre tableau, votre nom ou le lien d'invitation**. Collez-le dans votre message au service informatique. **Réessayer** refait la tentative ; **Fermer** ferme la fenêtre.

### Quoi dire au service informatique

- L'application a besoin d'une connexion HTTPS sortante (port 443, connexions de type WebSocket) vers le domaine indiqué dans le diagnostic.
- Tout est chiffré de bout en bout ; le relais ne peut pas lire les tableaux. Il voit les adresses IP et le volume de trafic.
- Si c'est trop pour votre politique, l'entreprise peut **héberger son propre relais** sur un domaine qu'elle maîtrise et autorise ([RELAY.md](RELAY.md)).
- Les connexions directes entre ordinateurs d'un même réseau ou VPN n'ont pas besoin du relais (mais demandent que l'UDP soit autorisé entre eux ; non vérifié sur un réseau d'entreprise).

### La limite, honnêtement

Le message rend l'échec compréhensible ; **il ne fait pas fonctionner la session**. Si votre réseau refuse le domaine du relais, la session ne marchera pas à travers lui, et Inkboard n'essaie pas de contourner les règles de votre entreprise. Il reste : un autre réseau (mais un agent de sécurité installé sur votre portable peut tout faire passer par l'entreprise, même sur la connexion de votre téléphone : non vérifié), un relais approuvé par l'entreprise, ou une connexion directe sur le réseau local ou le VPN de l'entreprise (non vérifié sur un réseau d'entreprise). Rappelez-vous aussi qu'envoyer des données de l'entreprise vers un service tiers peut être contraire à sa politique : dans le doute, demandez.

---

## Dépannage

| Ce que vous voyez | Ce que cela signifie / que faire |
|---|---|
| « Collez d'abord un lien d'invitation. » | Le champ du lien est vide. |
| « Ce n'est pas un lien d'invitation Inkboard. » / « Le lien est abîmé ou incomplet : demandez-en un nouveau. » | Le texte collé n'est pas un lien complet. Copiez-le de nouveau en entier (il est long). |
| « Ce texte est trop long pour être un lien d'invitation. » | Vous avez collé autre chose. |
| « Ce lien d'invitation n'est plus valable : demandez-en un nouveau. » | L'hôte a créé de nouveaux liens ou a terminé la session. Demandez un nouveau lien. |
| « La session est pleine. » | Le nombre maximum de participants de l'hôte est atteint. Demandez à l'hôte de l'augmenter. |
| « L'hôte a refusé votre demande. » | L'hôte a appuyé sur **Refuser**. |
| « Vous avez été exclu de cette session. » / « Vous avez été exclu de la session. » | L'hôte vous a exclu. |
| « L'hôte utilise une autre version d'Inkboard : mettez Inkboard à jour (ou rechargez la page). » | L'hôte et l'invité ont des versions qui ne se comprennent pas. Mettez à jour la plus ancienne. |
| « En attente de l'accord de l'hôte… » longtemps | L'hôte n'a pas encore accepté (la fenêtre n'a pas de limite de temps). Demandez-lui de regarder sa fenêtre Inkboard. |
| « Impossible de joindre le service de connexion… » | Voir [Réseaux restreints](#réseaux-restreints-entreprise-école). |
| « Reconnexion… » | Coupure temporaire. Patientez ; vos modifications sont conservées. |
| « La connexion a été perdue trop longtemps. » | La coupure a duré trop longtemps. Rejoignez de nouveau avec le lien (l'hôte devra peut-être vous accepter de nouveau). |
| « Vous avez un accès en lecture seule : cette modification a été refusée. » | Vous avez rejoint avec le lien de lecture, ou l'hôte vous a passé en lecture seule. |
| « Vous envoyez des modifications trop vite… » / « {nom} a dépassé une limite » | Un invité envoie trop, trop vite (un script, une touche bloquée). Des infractions répétées mènent à l'exclusion. |
| Un clic sur un lien `inkboard://` ne fait rien | Utilisez le menu **Rejoindre une session…** et collez le lien. |
| La connexion est lente ou le tableau met longtemps à arriver | Le trafic passe par le relais, dont le débit est limité quand il est public. Un relais à vous supprime cette limite. |
| L'hôte est introuvable sur un réseau local | Autorisez Inkboard dans le pare-feu Windows (réseaux privés, UDP compris) quand il le demande. Non vérifié sur deux vraies machines. |

## Questions fréquentes

**Les invités ont-ils besoin d'un compte ou d'installer quelque chose ?** Pas de compte. Pour l'instant, les invités ont besoin d'Inkboard (une version navigateur est prévue).

**Est-ce payant ?** Non. Par défaut, ce sont des relais publics gratuits (débit limité, pour le développement et les loisirs). Un relais à vous est facultatif.

**Où mon tableau est-il stocké ?** Uniquement sur votre ordinateur. Le relais ne stocke rien et ne peut rien lire.

**Plusieurs personnes peuvent-elles écrire dans le même texte en même temps ?** Pas caractère par caractère. La dernière modification reçue pour un objet l'emporte.

**Que devient mon propre tableau quand je rejoins une session ?** Il est mis de côté et rendu quand vous quittez. Rien de lui n'est envoyé à l'hôte.

**Les invités peuvent-ils modifier mes fichiers ?** Non. Ils ne modifient que le tableau affiché dans la session ; vous décidez de l'enregistrer ou non.

**Puis-je partager sans Internet ?** Sur un réseau local ou un VPN, les ordinateurs peuvent en principe se connecter directement sans relais. Cela n'a pas encore été vérifié sur deux vraies machines.

**Comment savoir qui est là ?** Le compteur vert « ● » dans la barre de titre et la liste Participants de la fenêtre Partager (hôte) ; les noms sur les curseurs (tout le monde).

**Puis-je partager deux tableaux à la fois ?** Non, un seul à la fois.

**J'ai cliqué sur « Arrêter le partage » : peut-on encore me joindre avec l'ancien lien ?** Non. Le lien n'est plus valable une fois la session terminée ; une nouvelle session a de nouveaux liens.
