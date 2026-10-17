# Distribution (Windows)

## Archive portable

```powershell
powershell -File scripts/package.ps1
```

Compile en release l'application (`tableau-blanc.exe`, icône incluse) la CLI (`wb.exe`) et le serveur MCP (`wb-mcp.exe`), puis produit
`dist/tableau-blanc-<version>-windows-x64.zip` et son empreinte SHA-256. L'archive contient aussi le README et les scripts ci-dessous.

## Installation par utilisateur (sans droits administrateur)

`install-user.ps1` (dans l'archive) copie les fichiers dans `%LOCALAPPDATA%\Programs\TableauBlanc`, crée un raccourci du menu Démarrer et
ajoute « Tableau blanc » au menu **Ouvrir avec** des fichiers `.drawio`, `.excalidraw`, `.canvas` et `.mmd` (uniquement sous `HKCU`).
Il ne change l'application par défaut d'aucune extension. `uninstall-user.ps1` annule tout et conserve les documents et `%APPDATA%\TableauBlanc`.

Ces scripts n'ont pas été exécutés sur une machine d'essai (seul `package.ps1` a été lancé).
Le format natif s'appelle `.tableau.json` : Windows associe les fichiers à leur *dernière* extension (`.json`), il n'est donc pas associé automatiquement.

## Signature et installateur

Un installateur signé (MSI/MSIX/Inno Setup) exige un **certificat de signature de code** (OV/EV), qui ne peut pas être fourni ici. Sans signature,
SmartScreen avertit au premier lancement. Une fois le certificat obtenu : signer `tableau-blanc.exe` et `wb.exe`
(`signtool sign /fd SHA256 /tr <horodatage> /td SHA256 ...`) avant de lancer `package.ps1`, puis signer l'installateur.

## Icône

`crates/wb-app/assets/icon.png` (fenêtre) et `icon.ico` (exécutable, intégré par `build.rs` via `embed-resource`) ; sans `rc.exe`, la compilation continue sans icône d'exécutable.
