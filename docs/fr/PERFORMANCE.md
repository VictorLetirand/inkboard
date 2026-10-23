# Performance avec beaucoup d'icônes

*English version: [PERFORMANCE.md](../PERFORMANCE.md)*

Les icônes sont des images SVG, coûteuses de trois façons : les rastériser, envoyer leurs pixels à la carte graphique, et dessiner des milliers de petits quadrilatères texturés. Inkboard garde un tableau fluide avec des dizaines de milliers d'icônes en faisant le moins possible de chacune.

## Comment ça marche

- **La résolution suit l'écran.** Un SVG est rastérisé à la puissance de deux juste au-dessus de la taille à laquelle il est affiché (64 à 4096 pixels, jamais au-delà de la limite de texture de la carte graphique) : une icône agrandie ou zoomée est nette, une petite est bon marché. Plusieurs niveaux d'une même icône peuvent coexister.
- **Les petites icônes partagent un atlas.** Les icônes affichées à 30 px ou moins sont rangées dans des pages de texture partagées de 2048 × 2048 : des milliers sont dessinées en un seul lot au lieu d'un appel de dessin chacune.
- **Les icônes minuscules sont des aplats.** Sous 9 points à l'écran, une icône est dessinée comme un carré uni de sa couleur moyenne (aucune texture) ; le niveau de détail à 3 pixels de chaque objet s'applique toujours en dessous.
- **Un budget de temps, pas un gel.** Au plus environ 5 ms par image sont consacrées à la rastérisation ; les icônes qui n'y tiennent pas attendent l'image suivante et montrent entre-temps la texture la plus proche déjà disponible (ou un gris de remplacement). Une première vue d'un énorme tableau se remplit en quelques images ; l'interface ne se bloque jamais.
- **La mémoire est bornée.** Les textures peu utilisées récemment sont libérées au-delà de 256 Mo ; l'atlas est partagé.
- **Les icônes répétées sont stockées une seule fois.** Ajouter une icône déjà présente sur le tableau réutilise son contenu (mêmes octets, même type) : un schéma qui utilise 200 fois une icône de base de données la stocke, la rastérise et l'envoie une seule fois, et le fichier reste petit.
- **Élagage.** Seuls les objets qui croisent la vue sont parcourus (index spatial), comme pour tous les autres objets.

## Le mesurer

```bash
inkboard --bench-icons 20000:3000      # 20 000 icônes sur le tableau, dont 3 000 différentes
```

Il construit le tableau à partir de la bibliothèque d'icônes et déroule une session scriptée — premières vues, vue fixe de l'ensemble, zoom avant jusqu'à 300 %, déplacement à 100 %, zoom arrière, agrandissement d'une icône — en enregistrant chaque image : le temps CPU du code d'image d'Inkboard, et l'**intervalle entre images** (qui inclut aussi le dessin et le rafraîchissement de l'écran). Il affiche, pour chaque phase, la médiane, les 95e et 99e centiles, la pire image, la part des images au-dessus de 16,7 ms (60 i/s) et de 33 ms, et combien de SVG ont été rastérisés et en combien de temps.

Les tests unitaires vérifient les mécanismes (niveaux en puissance de deux, limite de taille de texture, atlas partagé, temps de rastérisation borné par image, repli sur la texture la plus proche, une rastérisation par icône, ressources identiques stockées une fois) ; `cargo test --release -p inkboard-app -- --ignored every_library_icon_renders` rend chaque icône de la bibliothèque.

## Résultats

Version release, Windows 11, Intel Core i9-12900K, NVIDIA GeForce RTX 5070, fenêtre de 1360 × 860 points ; le plancher d'environ 5,5 ms est l'intervalle d'images d'une boucle de redessin au repos sur cette machine. **Intervalle** entre images en ms (médiane / 95e centile / pire) :

| Icônes (différentes) | Tout en vue | Zoom avant 300 % | Déplacement 100 % | Zoom arrière | Agrandir une icône |
|---|---|---|---|---|---|
| 1 000 (60) | 5,6 / 5,7 / 5,8 | 5,6 / 5,8 / 9,2 | 5,6 / 5,9 / 6,4 | 5,6 / 5,9 / 6,2 | 5,6 / 5,9 / 20 |
| 5 000 (2 000) | 5,6 / 5,8 / 6,0 | 5,6 / 8,6 / 10,7 | 5,6 / 5,8 / 10,7 | 5,6 / 7,4 / 8,3 | 5,6 / 5,8 / 14,7 |
| 20 000 (3 000) | 13,0 / 14,1 / 20 | 5,7 / 12,7 / 13,7 | 5,6 / 5,8 / 6,0 | 5,6 / 12,0 / 13,4 | 5,5 / 6,3 / 17,8 |
| 100 000 (5 000) | 19,5 / 25,2 / 80 | 5,7 / 15,6 / 20 | 5,6 / 5,8 / 6,0 | 5,6 / 13,5 / 17,5 | 28 / 31 / 40 |

En clair : jusqu'à 5 000 icônes, rien ne descend jamais sous 90 i/s ; avec 20 000 icônes toutes en vue, le tableau reste au-dessus de 60 i/s et chaque zoom, déplacement et redimensionnement reste fluide ; à 100 000 icônes toutes en vue, il tourne autour de 40–50 i/s, et zoomer ou se déplacer dans n'importe quelle partie redevient fluide. La case lente de la dernière ligne est le redimensionnement d'une icône sur un tableau de 100 000 objets dans le banc d'essai, où chaque pas est une modification validée qui reconstruit l'index de 100 000 objets (18 ms) ; le glissement d'une poignée à la souris n'a pas été mesuré à cette taille.

Avant ce travail, avec 20 000 icônes en vue, la même session tournait à environ 34 ms par image (29 i/s), et une icône agrandie était visiblement floue.
