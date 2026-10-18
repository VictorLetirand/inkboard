# Bibliothèque d'icônes

*English version: [ICONS.md](../ICONS.md)*

Inkboard embarque environ **28 800 icônes et formes** en 217 jeux, disponibles hors ligne avec le bouton **Bibliothèque d'icônes** de la barre d'outils (ou *Insérer → Bibliothèque d'icônes…*). On cherche par nom, ou on filtre par groupe et par jeu ; un clic dépose l'icône au centre de la vue. Ce sont de simples objets image SVG : on les déplace, redimensionne, pivote, copie et exporte en SVG / PNG / PDF / draw.io / Excalidraw comme n'importe quel objet.

| Groupe | Contenu | Source |
|---|---|---|
| **AWS** | icônes d'architecture (actuelles et de quatre générations précédentes), icônes 3D, groupes | draw.io |
| **Microsoft Azure** | icônes Azure 2021, Cloud and Enterprise, Dynamics 365, Active Directory | draw.io |
| **Microsoft Office** | serveurs, appareils, utilisateurs, concepts, nuages… | draw.io |
| **Google Cloud** | icônes de produits et de catégories | draw.io |
| **Cisco** | équipements réseau, architecture SAFE, équipements de baie | draw.io |
| **IBM** | icônes IBM Cloud et produits | draw.io |
| **Cloud and IT** | Kubernetes, Alibaba Cloud, Atlassian, Salesforce, SAP, OpenStack, VMware, Veeam, Citrix, réseau, équipements de baie, logos web | draw.io |
| **Diagram shapes** | formes de base, organigramme, BPMN, flèches, patterns d'intégration, plan de site, plan d'étage | draw.io |
| **Engineering** | symboles électriques, P&ID (procédés), puissance fluide | draw.io |
| **Mockups** | éléments de maquette, Android, iOS, Bootstrap, Google Material | draw.io |
| **Signs and symbols** | transports, santé, sécurité, sports, nature… | draw.io |
| **Brands** | 3 466 logos de marques dans leurs couleurs | [Simple Icons](https://simpleicons.org) |
| **General icons** | Tabler (5 184), Lucide (1 891), Material Design Icons (7 447) | MIT / ISC / Apache-2.0 |

## Couleurs

Les jeux monochromes (formes de diagramme, ingénierie, maquettes, icônes générales, beaucoup de jeux génériques) sont stockés en blanc et **teintés avec la couleur de contour de l'objet** : choisissez une autre couleur dans le panneau de style et l'icône suit, dans l'application comme dans tous les exports. Les jeux de fournisseurs (AWS, Azure, Google Cloud, Cisco…) et les logos de marques gardent leurs propres couleurs.

## Licences et marques

- Les bibliothèques draw.io sont sous **licence Apache 2.0** (jgraph/drawio) ; Simple Icons est en **CC0** ; Tabler est **MIT** ; Lucide est **ISC** ; Material Design Icons est **Apache-2.0**. Les textes des licences sont dans [THIRD_PARTY_LICENSES.md](../../THIRD_PARTY_LICENSES.md).
- Les logos et icônes de produits (AWS, Azure, Google Cloud, Cisco, IBM, les logos de marques…) sont des **marques ou la propriété de leurs détenteurs respectifs**. Ils sont fournis, comme dans draw.io, pour légender des schémas de ces produits ; Inkboard n'est ni affilié ni approuvé par eux, et vous restez responsable du respect de la charte de chaque détenteur quand vous publiez un schéma.
- Les bibliothèques propriétaires d'autres outils (par exemple celle de Miro) ne sont pas incluses : elles ne sont pas publiées sous une licence qui autorise leur redistribution.

## Reconstruire le paquet

La bibliothèque est un seul fichier, `crates/inkboard-app/assets/icons.bin` (environ 8 Mo, compressé en deflate, intégré à l'exécutable), construit par [`scripts/build_icon_library.py`](../../scripts/build_icon_library.py) à partir de clones superficiels des projets amont. Les commandes exactes et les commits utilisés sont dans la [version anglaise](../ICONS.md#rebuilding-the-pack).

draw.io fournit ses formes sous forme de *stencils* (instructions de dessin vectoriel) dans un encodage binaire compact à l'intérieur de `stencils.min.js` ; le script les décode et convertit chacun en SVG. `crates/inkboard-icons` lit le paquet (index, recherche, extraction du SVG) et `crates/inkboard-app/src/library.rs` est la palette.
