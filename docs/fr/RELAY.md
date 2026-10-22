# Héberger son propre relais

*English version: [../RELAY.md](../RELAY.md)*

Le partage en direct ([SHARING.md](SHARING.md)) a besoin d'un petit **relais** pour que deux ordinateurs puissent se trouver et, quand ils ne peuvent pas se joindre directement, échanger leurs données. Par défaut, Inkboard utilise les relais publics gratuits du projet iroh (n0). Vous pouvez héberger le vôtre, avec le programme officiel `iroh-relay`. Cette page explique quand c'est utile, comment l'installer et comment y connecter Inkboard.

> **État et honnêteté.** Les fichiers d'exemple de [`packaging/relay/`](../../packaging/relay/) n'ont été vérifiés que pour leur syntaxe. Une mise en service complète sur un vrai serveur, avec un vrai nom de domaine, un certificat et de vrais invités, n'a **pas** été essayée. Ce qui est « non vérifié » est signalé comme tel. Sources : la documentation d'iroh (docs.iroh.computer, pages « Self-hosted » et « Rate limiting », lues le 2026-10-22), le code source d'`iroh-relay` 1.4.0 et nos propres essais (spike S1).

## Ce qu'est un relais (et ce qu'il n'est pas)

- **Sans état** : il ne stocke rien ; il n'y a rien à sauvegarder. Il fait passer des octets entre des ordinateurs.
- **Il ne voit que des données chiffrées.** Les sessions Inkboard sont chiffrées de bout en bout. Un relais voit tout de même des métadonnées réseau : adresses IP, heures, volumes (comme le dit la documentation d'iroh pour les relais en général).
- **Ce n'est pas un serveur de session.** L'ordinateur de l'hôte reste le serveur de la session ; si l'hôte part, la session s'arrête.
- Dès que deux ordinateurs réussissent à se connecter directement, les données cessent de passer par le relais.

## Quand est-ce utile ?

- **Usage sérieux ou régulier.** Les relais publics sont, selon iroh, pour le « développement et les loisirs », sans garantie de service, limités en débit par connexion (limites non publiées ; nous avons mesuré de 5 à 17 Mbit/s) et pris en charge seulement pour la dernière version. Pour une équipe qui partage des tableaux tous les jours, votre propre relais supprime ces limites.
- **Une entreprise qui autorise un domaine précis.** Lors de notre essai sur un réseau d'entreprise avec une passerelle de sécurité dans le nuage, le domaine du relais public a été refusé (HTTP 403 avec une page d'avertissement, « catégorie inconnue ») et les sessions n'ont pas pu démarrer. Une entreprise peut héberger un relais sur un domaine qu'elle maîtrise et demander à son service informatique de l'autoriser, pour que ses équipes puissent partager des tableaux. C'est le seul remède : Inkboard n'essaie pas de contourner les règles d'un réseau.
- **Garder les métadonnées (adresses IP, heures) dans votre propre infrastructure.**

Si vous ne partagez qu'occasionnellement entre personnes sur des réseaux ouverts, les relais publics suffisent.

## Prérequis

- Une **machine avec une adresse IP publique** (un petit serveur privé virtuel suffit ; le relais fait passer des données mais calcule peu). La capacité sous charge réelle (20 personnes, grosses images) n'a pas été mesurée.
- Un **nom de domaine** (par exemple `relay.example.org`) avec un enregistrement A et/ou AAAA qui pointe vers la machine.
- **TCP 443** ouvert (le relais, en HTTPS et WebSocket) et **TCP 80** (le relais l'écoute par défaut). **UDP 7842** facultatif, seulement si vous activez la découverte d'adresse QUIC (voir plus bas).
- Pour utiliser les fichiers d'exemple : **Docker avec Compose**. Sans Docker, vous pouvez utiliser directement le programme `iroh-relay` : binaires sur la page des versions d'iroh, ou `cargo install iroh-relay --version 1.4.1 --features server --locked` (la fonctionnalité `server` est obligatoire ; lors de notre essai, nous avons compilé la version 1.4.0 ainsi).
- Les versions récentes d'Inkboard utilisent `iroh` 1.4.1. Gardez le relais dans la même série 1.x. Selon la documentation d'iroh, la série est 1.x, le protocole est rétrocompatible avec la version majeure précédente, et une version mineure est pleinement prise en charge environ trois mois.

## Installer avec le kit d'exemple

Le kit est dans `packaging/relay/` : `docker-compose.yml`, `relay.toml` et un court `README.md`.

1. Copiez le dossier `packaging/relay/` sur votre machine.
2. Modifiez `relay.toml` : mettez votre domaine dans `hostname` et votre adresse électronique dans `contact` (obligatoire pour Let's Encrypt).
3. Démarrez :
   ```
   docker compose up -d
   ```
4. Vérifiez qu'il répond (depuis une autre machine) :
   ```
   curl --noproxy '*' --fail --show-error https://relay.example.org/healthz
   ```

### TLS automatique (Let's Encrypt)

Avec `cert_mode = "LetsEncrypt"`, le relais obtient et renouvelle son certificat tout seul ; il faut que le nom de domaine pointe vers la machine et que le port 443 soit joignable depuis Internet (le code source du programme répond au défi « TLS-ALPN-01 » sur le 443). Le cache des certificats est conservé dans le volume `relay-certs` : il n'est pas précieux (le relais en redemanderait un), mais gardez-le pour ne pas atteindre les limites de Let's Encrypt si vous redémarrez souvent. N'utilisez `prod_tls = false` que pour des essais : cela choisit le service de test de Let's Encrypt, dont les certificats ne sont pas reconnus.

Le TLS est **indispensable en pratique** pour la future version navigateur d'Inkboard : une page web en `https://` ne peut pas ouvrir de WebSocket non chiffrée (contenu mixte ; non testé). L'application native n'est pas soumise à cette règle, mais un relais sans TLS (HTTP simple) n'est prévu que pour des essais locaux.

### Configuration (`relay.toml`)

Les options utilisées par l'exemple et leur rôle (vérifiés dans le code source d'`iroh-relay` 1.4.0) :

| Option | Signification |
|---|---|
| `http_bind_addr` | Adresse et port du côté HTTP simple (par défaut `[::]:80`). L'exemple utilise `0.0.0.0:80`. |
| `[tls]` `https_bind_addr` | Le relais lui-même (par défaut : même adresse, port 443). |
| `[tls]` `hostname`, `contact`, `cert_mode`, `cert_dir` | Domaine, courriel, `"LetsEncrypt"` (ou `"Manual"` avec vos propres fichiers de certificat, ou `"Reloading"`), dossier du cache des certificats. |
| `enable_quic_addr_discovery` | Désactivé par défaut. Activé (exige le TLS), il écoute sur UDP 7842 et indique aux ordinateurs leur adresse publique, ce qui les aide à trouver un chemin direct. Qu'Inkboard s'en serve avec un relais personnalisé n'est **pas vérifié** ; le relais fonctionne sans (nos essais). |
| `enable_metrics` | Activé par défaut, sur le port 9090. L'exemple le désactive ; si vous avez besoin des métriques, n'exposez jamais ce port sur Internet. |
| `[limits.client.rx]` | Limite de débit par connexion (voir la section suivante). |
| `access` | Contrôle d'accès (liste autorisée, liste interdite, jeton partagé, vérification HTTP). **Laissez la valeur par défaut** ; la compatibilité d'Inkboard avec ces restrictions n'est **pas vérifiée**. |

Le projet amont indique que la liste complète des options n'est pas publiée sous forme de documentation et renvoie au code source (`iroh-relay/src/main.rs`).

## Sécurité, limites de débit et abus

- **Toute personne qui connaît l'adresse de votre relais peut l'utiliser** comme relais (avec le contrôle d'accès par défaut) : le relais ne sait pas à qui appartient le trafic et ne peut pas le lire. Le risque est donc de la bande passante et du processeur consommés par des inconnus, pas des tableaux divulgués.
- **Les limites de débit sont désactivées par défaut** dans le programme. L'exemple fixe une limite par connexion dans `[limits.client.rx]` (`bytes_per_second`, `max_burst_bytes` facultatif) : le relais lit chaque connexion au plus à cette vitesse (les données sont ralenties, pas perdues ; les points de terminaison iroh journalisent un avertissement indiquant que le relais les limite). Ajustez la valeur à la bande passante de votre machine. Les limites sur les nouvelles connexions (`accept_conn_limit`, `accept_conn_burst`) n'ont, selon la documentation d'iroh, pas encore d'effet.
- Complétez avec le **plafond de bande passante et le pare-feu de votre hébergeur**, et surveillez le trafic.
- **Métriques désactivées ou privées**, comme dans l'exemple. Le conteneur s'exécute en root par défaut (l'image ne définit pas d'utilisateur) : l'exemple ajoute `no-new-privileges` ; gardez la machine à jour et, si possible, le relais seul dessus.
- **Journaux** : l'exemple limite la taille des journaux du conteneur. Le programme écrit sur la sortie d'erreur ; ce qu'il journalise par défaut (par exemple, si des adresses IP apparaissent) n'a **pas** été vérifié ici. Vérifiez-le avant de promettre quoi que ce soit à vos utilisateurs.
- Votre relais voit les adresses IP, les heures et les volumes : dites-le à vos utilisateurs, comme pour tout service.

## Sauvegarde

**Rien à sauvegarder.** Le relais ne contient ni tableau ni compte. Seul le cache des certificats est conservé, et il se reconstruit automatiquement. Gardez `relay.toml` (c'est votre configuration).

## Mise à jour

1. Changez l'étiquette de l'image dans `docker-compose.yml` (par exemple `v1.4.1` vers une étiquette 1.x plus récente).
2. `docker compose pull && docker compose up -d`.

Suivez les versions mineures (une mineure est pleinement prise en charge environ trois mois). Les sessions en cours sont coupées pendant le redémarrage et la « Reconnexion… » des invités prend le relais (une coupure de quelques secondes à une minute est absorbée, voir [SHARING.md](SHARING.md)) ; mettez plutôt à jour quand personne ne travaille.

## L'utiliser depuis Inkboard

Inkboard peut utiliser une **liste de relais personnalisée** à la place des relais publics. Quand vous hébergez une session avec votre relais, le lien que vous partagez porte l'adresse de votre relais : **les invités n'ont rien à configurer**, ils suivent le lien (vérifié lors de nos essais avec un relais local : l'invité prend son relais dans le lien). Si vos invités sont sur un réseau restreint, le domaine à faire autoriser par le service informatique est celui de votre relais (indiqué par la fenêtre d'erreur et par « Copier le diagnostic », voir [SHARING.md](SHARING.md#réseaux-restreints-entreprise-école)).

> **À compléter par le lot R.** Le réglage exact (où il se trouve, comment il s'appelle, comment la liste se saisit ou se stocke, comment revenir aux relais publics) est réalisé par un autre lot et n'est **volontairement pas décrit ici**. À décrire ici quand il existera : emplacement dans l'interface ou dans un fichier, format de la liste, valeur par défaut, et un test (démarrer une session, vérifier que le lien contient votre domaine).

Ce que la couche réseau du code gère déjà, pour savoir à quoi s'attendre : un réglage « relais par défaut / liste personnalisée / aucun relais » existe dans la bibliothèque réseau ; avec une liste personnalisée, les relais du lien sont utilisés aussi. Le fait que l'adresse de relais du lien soit arbitraire est un point examiné par la revue de sécurité.

## Limites (liste honnête)

- Non essayés : un vrai serveur, un vrai domaine, un vrai certificat Let's Encrypt, une charge de 20 personnes ou plus, le bouton de diagnostic face à votre relais à travers une vraie passerelle d'entreprise.
- Un relais ne répare pas un réseau qui refuse votre domaine : si l'entreprise ne l'autorise pas, la session ne marchera pas à travers ce réseau.
- Un relais n'est pas nécessaire sur un réseau local ou un VPN où les ordinateurs se joignent directement (non vérifié sur deux vraies machines).
- La version navigateur d'Inkboard (prévue) aura besoin d'un relais avec TLS.
