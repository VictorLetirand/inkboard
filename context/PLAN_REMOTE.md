# Plan — Session partagée (remote) : l'hôte est le serveur, un lien pour inviter

*Rédigé le 2026-10-21, après la discussion sur le travail à distance (Microsoft Whiteboard dans Teams) et le serveur MCP v2.*
*Base : `main` à `93138a5` (rc3 en cours de publication). Exécutants prévus : agents Sonnet 5.5 par lot (palier T1), avec des « spikes » de décision d'abord. Intégration et relecture : l'agent principal.*
*Ce document décide l'architecture, découpe le travail et liste les décisions qui restent à l'utilisateur (§12). Rien n'est encore implémenté.*

---

## 0. Cahier des charges (réponses de l'utilisateur)

- **Paliers 1 à 4 tous voulus** : (1) curseur de l'agent MCP en local ; (2) session entre postes ayant Inkboard, sur réseau local / VPN ; (3) session via Internet ; (4) invité dans un **navigateur**, sans installation.
- **Public visé : tout le monde**, pas seulement des collègues : des invités peuvent être hors du réseau de l'hôte, sur n'importe quelle machine.
- **Invité sans installation** : un lien ouvrable dans un navigateur est une exigence de produit, pas une option.
- **Taille des sessions** : l'utilisateur vise 2 à 5 personnes en pratique, mais veut qu'on **prévoie grand** (un nombre plus élevé, que son message n'a pas donné en entier). **Hypothèse de travail de ce plan : conçu et testé pour 20 participants simultanés, plafond configurable (défaut 10), charge de contrôle à 50 participants simulés.** À confirmer (D2).
- **L'hôte est le serveur** : pas de serveur applicatif à gérer et à payer. Seule exception admise, à valider (D1) : un **relais** sans état et sans accès au contenu (chiffré de bout en bout), remplaçable et auto-hébergeable.
- **Les curseurs des participants sont visibles**, y compris celui de l'**agent MCP** quand il agit.

## 1. Objectifs, non-objectifs, principes

**Objectifs**
1. Un hôte clique « Partager », reçoit un **lien** ; toute personne qui l'ouvre voit et modifie le même tableau en direct, avec les curseurs et les sélections des autres.
2. Le lien marche dans un **navigateur** (page statique) et dans l'application native (`inkboard://…`).
3. L'**agent MCP** apparaît comme un participant (curseur, halo sur les objets touchés), en session comme en local.
4. Aucune donnée du tableau n'est lisible par un tiers (relais compris) ; l'hôte contrôle qui entre et avec quels droits.

**Non-objectifs (cette fois)** : édition simultanée du *même texte* caractère par caractère (CRDT de texte) ; audio/vidéo ; historique serveur ; comptes utilisateurs ; hébergement de tableaux (l'hôte doit être en ligne : s'il part, la session s'arrête) ; client mobile complet (le web doit fonctionner au tactile pour voir et faire l'essentiel, sans promesse de parité) ; connexion d'un agent distant (seul l'agent de l'hôte agit, §4).

**Principes**
- **L'hôte fait autorité** (topologie en étoile) : il détient l'`Editor`, applique les modifications reçues dans l'ordre d'arrivée et rediffuse le résultat. Pas de fusion, pas de CRDT : les conflits sont tranchés par l'ordre d'arrivée à l'hôte.
- **Hors ligne par défaut** : rien ne se connecte tant que l'hôte n'a pas cliqué « Partager » ; l'application ne télécharge toujours rien.
- **Chaque palier est utile seul** ; on peut s'arrêter à n'importe lequel.
- **Un seul protocole** pour tous les clients (natif, web) et pour tous les transports.
- Compatibilité : le format de fichier `.inkboard` et le serveur MCP existant ne changent pas.

## 2. Architecture

```
 Invité web (WASM)      Invité natif                 Hôte (Inkboard)
 page statique ─┐       ┌─ inkboard://… ─┐     ┌───────────────────────────────┐
                │       │                │     │ Editor (autorité)             │
        chiffré ▼       ▼                │     │  ├─ apply_external(ops)       │
   ┌─────── Transport (iroh : QUIC direct ou relais) ─────►│  ├─ historique d'annulation │
   │                                      │     │  │   par auteur               │
   │   relais public ou auto-hébergé      │     │ Session (participants, droits,│
   │   (sans état, ne voit que du chiffré)│     │  validation, présence, assets)│
   └──────────────────────────────────────┘     │ MCP live (agent = participant)│
                                                └───────────────────────────────┘
```

Composants et crates :
- **`inkboard-session`** (nouveau, **pur Rust, compile en natif et en `wasm32`**) : types du protocole versionné, opérations sans index, codec, cryptographie du lien (§4), trait `Transport`, transport « mémoire » simulé (latence, perte, réordonnancement) pour les tests, logique du client (état optimiste, réconciliation, reconnexion).
- **`inkboard-core`** : `Editor::apply_external` (§3.4), historique par auteur, validation d'opérations. Aucune dépendance réseau.
- **`inkboard-app`** : module `session_host` (gestionnaire de participants, approbation, droits, diffusion, service d'images), interface (§7), rendu des curseurs distants (`presence.rs`), jonction avec `live.rs` (agent MCP).
- **`inkboard-web`** (nouveau, `cdylib` `wasm32`) : le client navigateur ; construit en CI et publié en **page statique** (GitHub Pages ou équivalent, sans serveur à gérer).
- **Transport par défaut : `iroh`** (QUIC chiffré de bout en bout, connexion directe par perçage de NAT, relais en secours, **navigateur supporté via relais uniquement**). Voir §5 pour les faits vérifiés et les réserves ; un spike décide avant tout engagement.

## 3. Protocole et cohérence

### 3.1 Messages (tous chiffrés ; version de protocole dans le premier message)
- `Hello { proto, name, join_proof, role_requested }` → `Welcome { participant, color, role, doc_meta, host_proto }` ou `Refused { reason }` (version, jeton, hôte plein, refus de l'hôte).
- `Snapshot { pages, objects, asset_manifest }` : le document **sans les octets des images** ; les images sont listées par empreinte (hash) et taille.
- `AssetRequest { hash }` / `AssetChunk { hash, offset, data, last }` : transfert à la demande, par morceaux (reprise possible), mise en cache chez l'invité ; une image déjà connue (même hash) n'est jamais renvoyée.
- Modifications : `Submit { client_seq, base_seq, ops }` (invité → hôte), `Applied { seq, author, client_seq?, ops }` (hôte → tous), `Rejected { client_seq, reason }`.
- Éphémères (non historisés) : `Transient { object, state }` (déplacement/redimensionnement/tracé en cours, ~20 Hz), `StrokePoints { stroke, points }`.
- Présence : `Presence { page, cursor?, selection?, tool?, editing? , viewport? }` (~20 Hz max, perte tolérée), `Agent { name, action, targets, at }`.
- Contrôle : `Follow { participant }`, `Kick`, `RoleChange`, `Bye { reason }`, `Ping/Pong`.

### 3.2 Modèle de cohérence (hôte-autorité, simple)
- **Opérations sans index** : on dérive de l'`Op` du cœur (`Insert{page,index,obj}`…) un `WireOp` indépendant de la position : `Put { page, object }` (insère ou remplace l'objet entier, par identifiant), `Delete { page, ids }`, `Order { page, ids }` (liste complète d'identifiants dans l'ordre d'empilement), `AddPage/RenamePage/RemovePage`. L'empilement ne dépend plus d'un index fragile.
- **Dernier arrivé à l'hôte gagne, par objet** (granularité de l'objet entier, comme Excalidraw). Chaque objet porte un `version` incrémenté par l'hôte ; `Submit` indique la `base_seq` connue ; si l'objet a changé depuis, l'hôte applique quand même (dernier gagne) mais le signale (`superseded`) pour que l'invité puisse prévenir discrètement.
- **Application optimiste** chez l'invité : il applique tout de suite sa modification, la garde dans une file « en attente », et à chaque `Applied` de l'hôte : annule son optimiste, applique l'état hôte, rejoue ce qui reste en attente. L'état de l'hôte l'emporte toujours (léger clignotement possible, jamais de divergence durable).
- **Convergence** : tous les invités rejouent le même flux ordonné `Applied` à partir du même `Snapshot` ⇒ même état que l'hôte (propriété testée, §10).

### 3.3 Modifications en cours (fluidité)
Déplacer, redimensionner ou tracer ne passe pas par l'historique pendant le geste (comme `set_live` en local) : l'invité envoie des `Transient` (et `StrokePoints`) à ~20 Hz, l'hôte les relaie, les autres affichent l'état intermédiaire ; au relâchement, **un seul** `Submit` valide le résultat (un pas d'annulation).

### 3.4 Annulation (le vrai chantier de conception)
- Les modifications distantes sont appliquées par `Editor::apply_external(ops)` : **elles ne passent pas dans la pile d'annulation locale de l'hôte** (sinon Ctrl+Z de l'hôte annulerait le travail d'un invité).
- L'hôte garde **une pile d'annulation par auteur** (invités et agent) : pour chaque `Submit`, les objets tels qu'ils étaient avant. `Undo` d'un auteur restaure l'état « avant » **sauf pour les objets modifiés depuis par quelqu'un d'autre** : ceux-là sont ignorés et l'auteur est prévenu (« 2 objets modifiés par Anna, non annulés »). L'hôte local garde son comportement actuel (sa pile à lui, inchangée).
- Les invités envoient `Undo/Redo` comme des commandes, l'hôte répond par des `Applied` ordinaires.

### 3.5 Reconnexion, départ de l'hôte, versions
- Reconnexion : l'invité redonne son identifiant de participant et son dernier `seq` ; l'hôte renvoie les `Applied` manquants (fenêtre de 1 000 ou 5 minutes) sinon un `Snapshot`. L'hôte garde la place 60 s.
- L'hôte ferme ou perd le réseau > 60 s : `Bye(host_left)` ; l'invité peut **télécharger une copie** du tableau (`.inkboard`) depuis son navigateur ou l'enregistrer en natif.
- `proto` incompatible : message clair (« mettez à jour Inkboard / rechargez la page »), jamais de comportement indéfini.
- Taille : transfert initial proportionnel au contenu ; les icônes SVG et images sont des `assets` dédupliqués par hash (une icône posée 100 fois = un envoi).

## 4. Sécurité

**Modèle de menace** : (a) un inconnu qui devine ou intercepte le lien ; (b) un invité malveillant qui envoie des opérations pour planter ou saturer l'hôte ; (c) un relais curieux ou compromis ; (d) l'agent MCP détourné qui tenterait de gérer le partage.

**Mesures**
- **Lien = secret** : `https://<page>#<ticket>` où le ticket (identité de l'hôte, indications de relais, **clé de session aléatoire**, rôle) est dans le **fragment** (jamais envoyé à un serveur, jamais journalisé). Deux liens distincts : **édition** et **lecture seule**. Le lien est invalidé à la fin de la session ; l'hôte peut le **régénérer** (révoque l'ancien).
- **Chiffrement de bout en bout** : celui du transport (iroh/QUIC) **plus** un chiffrement applicatif (AEAD, clé dérivée de la clé de session) pour ne dépendre d'aucune propriété du transport ; le relais ne voit que des octets chiffrés (et des métadonnées réseau, ex. adresses IP).
- **Accord de l'hôte** : chaque nouvel arrivant déclenche « *Anna veut rejoindre — Accepter en édition / en lecture / Refuser* » (option « accepter automatiquement en lecture »). Liste des participants avec **Exclure** et changement de droit à tout moment.
- **Droits** : hôte, éditeur, lecteur ; les opérations d'un lecteur sont refusées côté hôte (pas seulement masquées dans l'interface).
- **Validation côté hôte de tout ce qui vient d'un invité** : taille de message, nombre d'objets par message, bornes numériques (positions, tailles, épaisseurs), longueur des textes, taille de chaque image (15 Mio comme le MCP) et **quota par invité** (ex. 100 Mio), débit (opérations/s, éphémères/s), nombre de pages. Un invité qui dépasse est averti puis exclu. Jamais d'accès au système de fichiers de l'hôte ni d'ouverture de chemin.
- **Plafond** de participants (défaut 10, configurable) et d'invitations en attente.
- **L'agent MCP ne peut pas gérer le partage** : aucun outil MCP ne démarre, arrête ni configure une session ; l'agent est un participant d'**édition** de l'hôte, rien de plus. (Test dédié : la liste des outils ne contient rien de tel.)
- Revue de sécurité (`/security-review`) obligatoire avant le palier 3 (Internet) et avant le palier 4 (web).

## 5. Transport et joignabilité

**Choix recommandé : `iroh`** pour tous les cas (LAN, Internet, navigateur), derrière un trait `Transport` pour pouvoir le remplacer.
- *Faits vérifiés* (documentation d'iroh, 2026-10) : connexions directes par perçage de NAT avec **relais en secours** ; le trafic reste chiffré de bout en bout, le relais ne peut pas le lire ; **navigateur supporté en WebAssembly mais uniquement via relais** (pas de perçage, pas d'UDP) ; les relais publics de n0 sont **gratuits pour le développement et les tests mais limités en débit** ; le code du relais est **ouvert et auto-hébergeable** (IP publique + nom DNS, TLS automatique) ; une offre gérée existe.
- *Réserves* : le support navigateur était annoncé en alpha (versions 0.3x) ; l'API d'`iroh` évolue vite (avant 1.0) ; poids du WASM ; débit des relais publics. D'où le spike **S1** qui tranche (critères au §9).
- **Conséquence importante** : pour l'invité **navigateur**, il faut une **page web statique** hébergée quelque part. Elle ne demande aucun serveur applicatif : un hébergeur de pages statiques (GitHub Pages) suffit. Le ticket est dans le fragment de l'URL : la page le lit en local et se connecte à l'hôte via le relais.
- **Réseau local sans Internet** (cas LAN pur) : le ticket contient les adresses locales de l'hôte ⇒ connexion directe ; l'invité **natif** n'a besoin d'aucun relais. Pour un invité **navigateur** hors Internet, l'hôte pourrait servir la page lui-même (optionnel, hors lot initial).
- **Le « serveur » résiduel** : relais publics de n0 (gratuits, limités) pour commencer ; pour un usage sérieux, **un relais** auto-hébergé (petit VPS, sans état, ne stocke rien) ou l'offre gérée d'iroh. À décider (D1).
- **Plan B** si le spike échoue : `Transport` WebSocket (client sur la page web, hôte atteint par un petit relais WebSocket maison `inkboard-relay`, sans état, que chacun peut auto-héberger) + connexion TCP directe sur LAN. Même protocole, même chiffrement applicatif.
- **Réseaux d'entreprise stricts** : une minorité bloquera le perçage et/ou les relais inconnus ; le message d'erreur doit expliquer (« réseau trop restrictif : essayez un autre réseau ou le partage sur le réseau local »). Pas de contournement.

## 6. Client web (palier 4)

**Faisabilité (vérifiée sur le code)** : `inkboard-core`, `inkboard-icons` (`miniz_oxide`) et `inkboard-formats` (`resvg`, `quick-xml`…) sont du Rust pur, à priori compilables en `wasm32`. L'application utilise `eframe` + `wgpu` (WebGPU par défaut sur le web, WebGL activable) et a une vingtaine de points natifs (`#[cfg]`) : `arboard` (presse-papiers), `rfd` (dialogues de fichiers, supporte le web en asynchrone), `live.rs` (serveur TCP local pour le MCP), `watch.rs` (surveillance de fichier), `inkboard-pen` (stylet Windows).

**Stratégie** : compiler **l'application elle-même** pour le web avec des `cfg(target_arch = "wasm32")` autour des modules natifs, plutôt que de réécrire une interface parallèle qui divergerait. Le spike **S2** mesure l'effort réel avant d'engager.

**Points spécifiques au web**
- **Icônes** : le pack de 8 Mo n'est pas embarqué dans la page ; chargé à la demande depuis la page statique (fichier séparé, mis en cache) pour la palette ; les images du tableau viennent par la session (§3).
- **Polices** : l'hôte natif utilise les polices du système (Segoe UI, Consolas…), le web des polices embarquées ⇒ **les retours à la ligne peuvent différer** entre hôte et invité. Parade : un **jeu de polices embarqué commun** utilisé dans les sessions (lot H), à défaut ce défaut est documenté. *Risque connu.*
- **Entrées** : souris, **tactile**, stylet via événements de pointeur ; raccourcis clavier ; **presse-papiers** (copier/coller d'images : l'API web est asynchrone et exige un drapeau de compilation `web_sys_unstable_apis` selon la documentation d'`eframe`) ; **dépôt de fichiers** (le web fournit les octets, pas un chemin : `dropped_files` doit gérer les deux).
- **Rendu** : WebGPU quand disponible, repli WebGL ; budget de rendu des icônes inchangé ; test de performance sur 5 000 icônes.
- **Navigateurs visés** : versions récentes de Chrome, Edge, Firefox, Safari ; le reste « au mieux ».
- **Enregistrement** : l'invité peut **télécharger une copie** du tableau ; pas d'écriture sur le disque de l'hôte.
- **Taille** : budget indicatif de la page < 15 Mo compressés (hors icônes), mesuré par le spike ; `glow` au lieu de `wgpu` réduit le poids mais il faut vérifier le rendu.

## 7. Interface

- **Barre / menu : « Partager »** → fenêtre : état de la session, **lien d'édition** et **lien de lecture** (boutons Copier), régénérer le lien, plafond, « accepter automatiquement en lecture », **Arrêter le partage**. Indicateur permanent dans la barre (pastille + nombre de participants) tant qu'une session est ouverte.
- **Liste des participants** : nom, couleur, droit, indicateur « édite … », actions Exclure / changer le droit / **Suivre** (la vue suit ce participant, comme « suis-moi »).
- **Curseurs distants** : flèche colorée + étiquette de nom, taille constante à l'écran, interpolation fluide, disparition après inactivité (≈ 5 s), contour de sélection de la couleur du participant, badge « édite le texte de … » sur l'objet en cours d'édition (verrou *doux* : informatif, pas bloquant).
- **Invité** : écran de rejoint (nom, état « en attente d'acceptation par l'hôte »), bandeau « connecté à … », bouton Quitter, message clair en cas d'échec (§5), copie téléchargeable.
- Traductions françaises pour toute chaîne neuve (le projet a un test de complétude).

## 8. Curseur de l'agent MCP (palier 1, utile sans réseau)

L'agent agit déjà dans l'application de l'hôte (mode direct). On y branche la **présence** :
- à chaque appel d'outil, l'application émet un événement `Agent { name: "Claude" (nom du client si connu), action, targets, at }` : un **curseur** (couleur propre, badge « Claude ») glisse jusqu'au centre des objets créés/modifiés avec une **trace** courte et un **halo** de 1,5 s sur les objets touchés ; un `batch` produit une séquence rapide, un `render`/`describe_board` un simple « regard » (pulsation discrète sans déplacement) ;
- animation désactivable dans les paramètres ; aucun effet sur l'historique ni sur les sorties MCP ; sans fenêtre visible (tests, `--screenshot`) l'animation est sautée.
- Le même événement est **retransmis aux invités** en session (palier 2+) : ils voient l'agent agir.

## 9. Lots, vagues, dépendances

**Règles communes** (reprises de `context/PLAN_MCP_V2.md` §2.4) : code et commentaires en anglais ; diff minimal ; pas de refactor non demandé ; Write/Edit plutôt que heredoc ; `cargo test -p x -- nom` ; pas de `--release --workspace` ; portes de sortie : `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` (stable **et** `+1.99.1`), `cargo test --workspace` ; chaque lot sur sa branche depuis la branche d'intégration `feat/remote`, commit avec l'identité du dépôt et le trailer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`, **pas de push** ; rapport ≤ 25 lignes (fichiers, décisions, tests, non vérifié) ; aucune assertion de durée (CI Windows) ; toute chaîne neuve traduite dans `i18n_fr.rs`.

### Vague 0 — décisions et fondations (en parallèle)

| Lot | Contenu | Livrable | Critères de décision / de fin |
|---|---|---|---|
| **S1 — Spike transport** (T1, lecture + prototype jetable dans `spikes/`) | Prototype d'un hôte natif et de deux invités (natif et **navigateur WASM**) avec `iroh` : poignée de main, échange de 1 000 messages, 5 Mo de données, mesures de latence et de débit ; ticket dans le fragment d'URL ; test avec les relais publics de n0 ; essai d'un relais auto-hébergé en local | `context/mcp-remote/spike-s1.md` : verdict **iroh retenu / plan B WebSocket**, versions testées, poids du WASM, limites constatées, licences des dépendances (compatibles MIT/Apache, `cargo deny`) | iroh retenu si : le navigateur se connecte via relais, latence médiane d'un message < 150 ms sur relais public, WASM < 6 Mo avant compression, licences OK, API stable sur les versions testées. Sinon plan B |
| **S2 — Spike WASM** (T1) | Tenter `cargo build --target wasm32-unknown-unknown` de `inkboard-core`, `-icons`, `-formats`, puis de `inkboard-app` avec `cfg` provisoires ; dresser la liste **exhaustive** des blocages (modules natifs, dépendances), mesurer le poids, afficher un tableau vide + une forme dans une page `trunk` | `context/mcp-remote/spike-s2.md` : liste des blocages avec estimation (S/M/L), poids, WebGPU vs WebGL, recommandation (cfg-gating de `inkboard-app` ou extraction d'un crate d'interface) | Décide la stratégie du lot F |
| **A — Présence et curseur de l'agent (palier 1)** | `presence.rs` (modèle `Participant`, `PresenceState`, rendu des curseurs/halos/traces/étiquettes, interpolation), branchement dans `live.rs` pour émettre `Agent` à chaque appel d'outil, réglage « animer l'agent » | PR dans `inkboard-app` ; captures d'écran (`--screenshot`) du curseur de l'agent, du halo et d'un `batch` | tests d'interface (kittest) : l'événement est émis pour chaque outil (lecture seule comprise), l'historique et la révision ne changent pas, sans animation en mode capture ; aucune régression du mode direct |
| **B — `inkboard-session` (protocole, chiffrement, transport simulé)** | Nouveau crate pur Rust (natif + `wasm32`) : messages versionnés, `WireOp` et conversion depuis/vers `Op` (ordre par identifiants), codec, AEAD + format du ticket/lien, trait `Transport`, transport mémoire avec latence/perte/réordonnancement, **client optimiste** (file en attente, réconciliation), validation d'entrées | crate + tests | propriétés : (1) pour des séquences aléatoires d'opérations de N invités, tous convergent vers l'état de l'hôte ; (2) tout octet aléatoire décodé ne panique jamais ; (3) ticket altéré refusé ; (4) compile pour `wasm32-unknown-unknown` ; (5) messages trop gros / bornes dépassées refusés |

### Vague 1 (après S1, S2 et B)

| Lot | Contenu | Dépend de | Critères |
|---|---|---|---|
| **C — Hôte (palier 2, côté hôte)** | `Editor::apply_external` + historique par auteur (§3.4) dans `inkboard-core` ; module `session_host` (participants, approbation, droits, plafonds, quotas, relais des éphémères, service d'images par hash et par morceaux, `Snapshot`) ; interface « Partager » (§7) ; transport retenu par S1 côté hôte ; événements `Agent` retransmis (lot A) | A, B, S1 | tests avec transport simulé : N invités simulés (jusqu'à 50), l'annulation d'un invité n'affecte pas la pile de l'hôte ni l'objet modifié depuis par un autre, droits de lecture refusés côté hôte, quotas appliqués, hôte plein refusé, aucun outil MCP de gestion du partage ; `Ctrl+Z` de l'hôte inchangé |
| **D — Invité natif (palier 2, côté invité)** | Jonction par lien/ticket (`inkboard://`, enregistrement du schéma dans les installeurs NSIS/MSI/deb/dmg), écran de rejoint, état optimiste, curseurs distants (réutilise A), reconnexion, copie locale à la sortie de l'hôte | B, C | test de bout en bout avec les vrais binaires (hôte + invité) en boucle locale : édition croisée, reconnexion, départ de l'hôte, version incompatible |

### Vague 2 (palier 3 et 4)

| Lot | Contenu | Dépend de | Critères |
|---|---|---|---|
| **E — Joignabilité Internet (palier 3)** | Configuration du relais (liste d'URL, défaut), messages d'erreur réseau explicites, option relais personnalisé dans les paramètres ; si plan B : crate/binaire `inkboard-relay` (WebSocket, sans état, limites de débit et de taille, sans journal) + documentation d'auto-hébergement ; revue de sécurité | S1, C, D | test de contrôle par processus séparés ; charge de 20 participants simulés sur relais local ; **revue de sécurité sans point bloquant** ; documentation « héberger son relais » |
| **F — Client web (palier 4)** | Selon S2 : `cfg`-gating de `inkboard-app` ou crate d'interface ; crate `inkboard-web` (+ configuration `trunk`/`wasm-bindgen`), transport web, chargement différé du pack d'icônes, entrées tactile/stylet/presse-papiers/dépôt par octets, copie téléchargeable, **construction et publication de la page statique par la CI** | S2, E | la page rejoint une session d'un hôte natif sur navigateur Chrome, Edge, Firefox (essais manuels journalisés) ; test automatisé de la logique client en `wasm-bindgen-test` ; poids de la page mesuré ; aucun accès au disque ; revue de sécurité du web |
| **H — Cohérence d'affichage et charge** | Jeu de polices commun embarqué pour les sessions (corrige les retours à la ligne différents), campagne de charge (20 et 50 participants simulés, tableaux de 5 000 icônes) | C, F | écarts de mise en page hôte/invité mesurés avant/après ; débit et latence consignés (sans assertion de durée dans la CI) |
| **G — Documentation, packaging, publication** | `docs/` (EN + FR) : partager, sécurité, héberger son relais, limites ; pare-feu Windows ; notes de version ; mise à jour de `README*` ; contrôle `cargo deny` | tous | docs complètes, tests de documentation, `deny` vert |

**Ordre de fusion** : A, B, S1, S2 (en parallèle) → C → D → E → F → H → G. Chaque fusion dans `feat/remote` ; **aucune publication sur `main` sans accord**.

## 10. Stratégie de tests

- **Réseau simulé en mémoire** (transport « mémoire » avec latence, perte, réordonnancement) pour tout le protocole : déterministe, rapide, sans port ni horloge.
- **Propriétés** (`proptest`) : convergence, annulation par auteur (jamais d'effet sur les autres objets), décodage robuste, validation de bornes, round-trip `Op` ↔ `WireOp`.
- **Sécurité** : jeton/ticket invalides, clé fausse, message géant, flot d'éphémères, image trop grande, quota dépassé, lecteur qui tente d'écrire, invité exclu qui retente, outil MCP de partage inexistant.
- **Bout en bout** avec les vrais binaires (hôte + invité natif) en boucle locale ; **web** : `wasm-bindgen-test` pour la logique, essais manuels journalisés sur navigateurs réels (la CI ne les couvre pas).
- **Visuel** : captures de l'application (curseurs distants, halo de l'agent, liste des participants, fenêtre de partage) comparées comme les tests visuels existants.
- **Charge** : simulateurs de participants (20 et 50), tableaux de 5 000 icônes, mesures consignées ; aucune assertion de durée.
- **Non automatisable ici (à écrire dans le journal sans prétendre le contraire)** : vrais NAT d'entreprise, réseaux restrictifs, navigateurs et appareils variés, relais public réel sous charge.

## 11. Risques

| Risque | Parade |
|---|---|
| `iroh` : API instable, support navigateur encore jeune, relais publics limités | Spike S1 avec critères chiffrés ; trait `Transport` ; plan B WebSocket ; relais auto-hébergeable |
| Le port WASM de l'application est plus gros que prévu | Spike S2 avant tout engagement ; le palier 4 peut glisser sans bloquer 1 à 3 |
| Retours à la ligne différents hôte / invité (polices) | Lot H (jeu de polices embarqué commun) ; documenté en attendant |
| Complexité de l'annulation par auteur | Règle simple (restaurer sauf objets modifiés depuis), propriétés testées, message clair à l'auteur |
| Invité malveillant ou trop gourmand | Validation et quotas côté hôte, exclusion, plafonds, tests de sécurité |
| Réseaux d'entreprise qui bloquent | Message explicatif, mode réseau local, pas de contournement |
| Abus d'un relais public | Limites de débit/taille, sans journal ; relais auto-hébergeable pour un usage sérieux |
| Poids de l'instantané pour un gros tableau | Images par empreinte et à la demande ; instantané compressé ; test avec 5 000 icônes |
| Lien divulgué | Secret dans le fragment, accord de l'hôte pour chaque arrivant, liens édition/lecture séparés, régénération |
| Dérive entre versions d'Inkboard | Version de protocole dans le premier message, refus explicite |

## 12. Décisions à prendre par l'utilisateur

- **D1 — Relais** : (a) relais publics de n0 pour commencer, relais auto-hébergé ensuite si l'usage devient réel (**recommandé**, coût nul au départ) ; (b) relais auto-hébergé dès le départ (petit VPS, quelques euros par mois, à qui l'on confie la maintenance) ; (c) offre gérée d'`iroh`. À trancher **après le spike S1**.
- **D2 — Taille des sessions** : confirmer **20 participants de conception**, plafond par défaut **10**, charge de contrôle **50** (le nombre de ton message était manquant).
- **D3 — Page web** : où publier la page statique (GitHub Pages sous le dépôt, ou un domaine à toi) et sous quel nom visible dans le lien.
- **D4 — Nom affiché de l'agent** : « Claude » ou le nom du client MCP connecté (« Copilot », etc.), ou « Agent ».
- **D5 — Valeurs par défaut de confiance** : accord de l'hôte pour chaque arrivant (recommandé), acceptation automatique en lecture désactivée par défaut.
- **D6 — Licence et dépendances** : feu vert pour ajouter `iroh` et ses dépendances (vérifiées par `cargo deny`) si S1 conclut positivement.

## 13. Hors périmètre / suites possibles

Texte co-édité caractère par caractère (CRDT de texte) ; enregistrement et relecture d'une session ; audio/vidéo ; intégration à Microsoft Teams (application de réunion) et à Copilot Studio (serveur MCP distant) ; hébergement persistant de tableaux ; comptes ; agent distant participant (un invité qui amène son propre agent).

## 14. Journal

**2026-10-21 — vague 0 (A, B, S1, S2) et tests autonomes.** Branche `feat/remote` (curseur de l'agent, crate `inkboard-session`, spikes S1 et S2 + leurs rapports dans `context/mcp-remote/`).
- **Portes de sortie** sur `feat/remote` : `cargo fmt`, `clippy -D warnings` (stable et 1.99.1), `cargo test --workspace` : 586 tests, 0 échec.
- **S1 rejoué ici** : direct 0,07 ms ; relais public n0 49 ms médiane, 5–16 Mbit/s (message de limitation de débit observé) ; **invité navigateur** (Chromium intégré de Claude, pas Chrome 155) via relais public : 49,6 ms médiane, 17/15 Mbit/s ; **page servie en http sur une adresse locale (contexte non sécurisé) : l'invité rejoint** (pas besoin de HTTPS pour la page).
- **S2 vérifié dans le navigateur intégré** : l'application web s'affiche (français, formes, outils), une forme se dessine à la souris, l'éditeur de texte s'ouvre et accepte la saisie. **Défauts constatés** : le texte saisi dans un rectangle déborde à droite de la forme au lieu de rester dans le cadre ; `Échap` n'a pas validé la saisie dans cet essai. À investiguer au lot F (build provisoire, polices ou événements clavier web).
- **Curseur de l'agent en vraie fenêtre** : batch MCP envoyé à l'application native ouverte, captures prises pendant l'animation : curseur « Claude », halos sur les objets créés, bandeau « Agent : batch » visibles. Limite : le tableau chargé était un ancien tableau de test à 11 % de zoom (formes minuscules).
- **Charge du protocole** : 50 et 100 invités simulés (300 modifications, 20 % de perte, 10 % de duplication, 30 % de réordonnancement) convergent vers l'état de l'hôte (tests dans `inkboard-session/tests/session.rs`).
- **Paquet de test manuel** (hors dépôt, `out/remote-test.zip`, scripts + `LISEZMOI.md`) pour essayer depuis le PC d'entreprise et un téléphone.
- **Essais réseau réels (voir `context/mcp-remote/tests-reseau-reels-2026-10-21.md`)** : invité Linux (homelab, même box) ↔ hôte Windows dans les deux sens via relais public (49 ms) ; **invité navigateur sur téléphone Android en 5G** via relais public (médiane 82–88 ms, 59 ms en Wi-Fi) ; coupures : survit à ~5 s et à un passage mobile/Wi-Fi ; **perdue à 20 s avec le délai d'inactivité par défaut (30 s)** ; **survit à 20 s et 60 s avec un délai de 120 s** (pire blocage 34 s et 90 s). Décision : délai d'inactivité de **120 s** des deux côtés + reconnexion applicative + accélération de la reprise (temps de reprise croissant avec la durée de la coupure).
- **Reste non vérifié** : tout réseau autre que ce poste (NAT, entreprise, mobile), Firefox/Safari/Edge, relais auto-hébergé avec TLS, intégration de l'hôte dans l'application (lots C, D), annulation par auteur (conception seulement).
