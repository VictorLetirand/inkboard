# Tester Inkboard en 10 minutes
*English version: [TESTING.md](../TESTING.md)*

Merci d'essayer Inkboard ! Les versions actuelles sont des **release candidates** (`v0.2.0-rc.N`) : elles n'ont été utilisées à la main que sous Windows.
Les versions Linux et macOS passent les tests automatiques mais personne ne les a encore essayées à la main, et le stylet n'a pas été validé sur du vrai matériel.
Chaque retour aide, qu'il dise « tout va bien » ou « ça a planté ».

## 1. Le récupérer

Téléchargez l'installeur de votre système sur la page [**Releases**](https://github.com/VictorLetirand/inkboard/releases) (la dernière `rc`) :

| Système | Fichier |
|---|---|
| Windows 10/11 | `inkboard_*_x64-setup.exe` |
| Linux (x86-64) | `inkboard_*_amd64.deb` ou `inkboard_*_x86_64.AppImage` |
| macOS | `Inkboard_*.dmg` |

Les installeurs **ne sont pas encore signés** : votre système vous avertit au premier lancement. Sous **Windows**, SmartScreen affiche « Windows a protégé votre ordinateur » → *Informations complémentaires* → *Exécuter quand même* ;
sous **macOS**, Ctrl-clic sur l'application → *Ouvrir*. Chaque version publie des sommes SHA-256. Sans rien installer : rejoignez un tableau partagé depuis une page web (voir l'étape 8).

## 2. À essayer

1. **Stylet :** tracez quelques traits à main levée. Dans le panneau de style du stylo, donnez à un trait une **pointe de flèche à la fin**.
2. **Formes et texte :** dessinez un rectangle, une ellipse et un losange. Le texte s'ouvre juste après le dessin de la forme : saisissez-y du texte.
3. **Connecteurs :** reliez deux formes par une flèche, puis déplacez une forme : le connecteur doit suivre.
4. **Tableau :** insérez un tableau, remplissez quelques cellules, ajoutez une ligne.
5. **Coller depuis un tableur :** copiez quelques cellules dans Excel / LibreOffice / Google Sheets et collez-les sur le tableau.
6. **Export :** exportez le tableau en **PNG** et en **PDF**, puis ouvrez les résultats.
7. **Ouvrir un fichier d'un autre outil :** ouvrez un fichier draw.io ou Excalidraw que vous avez déjà (`.drawio`, `.excalidraw`).
8. **Partage en direct :** cliquez sur *Partager* et rejoignez depuis un second appareil ou un navigateur (le lien d'invitation s'ouvre dans une page web, rien à installer). Chacun voit-il les modifications et le curseur de l'autre ?
9. **Thème sombre :** passez au thème sombre depuis le menu et regardez autour de vous.

## 3. Si vous avez le bon système ou le bon appareil

- **Linux :** essayez l'AppImage (`chmod +x *.AppImage && ./*.AppImage`) et le `.deb` (`sudo apt install ./inkboard_*.deb`). Indiquez votre distribution et si vous êtes sous X11 ou Wayland.
- **macOS :** essayez le `.dmg` (Apple Silicon ou Intel) et dites lequel.
- **Surface / stylet :** lancez `inkboard --pen-test`, vérifiez la pression et la gomme, puis envoyez le rapport ([comment](STYLET.md)).

## 4. Racontez-nous

- Remplissez le formulaire [**Test report**](https://github.com/VictorLetirand/inkboard/issues/new?template=test_report.md) (en anglais ou en français) : version, système, ce qui a marché, ce qui n'a pas marché (captures bienvenues).
- Questions, idées, ou simplement « ça marche très bien » : [GitHub Discussions](https://github.com/VictorLetirand/inkboard/discussions).
- Si Inkboard plante, il écrit un journal et indique où : `%APPDATA%\Inkboard\crash.log` sous Windows. Merci de le joindre.
