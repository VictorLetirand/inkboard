# Valider le stylet sur du matériel réel

La pression et la gomme du stylet sont **implémentées et testées automatiquement, mais jamais essayées avec un vrai stylet** (aucun numériseur n'était
disponible pendant le développement). Le mode diagnostic permet de le vérifier en deux minutes.

## Procédure (Windows)

```bash
inkboard --pen-test
```

Un panneau « Diagnostic du stylet » s'affiche à droite. Sur un appareil à stylet (Surface, tablette Wacom, écran tactile à stylet…) :

1. **Dessinez** avec l'outil Crayon en variant la pression : le trait doit s'épaissir quand vous appuyez. Dans le panneau : « Stylet détecté par Windows »,
   « Pression variable » et « Pression reçue par l'application (egui) » doivent passer à **OK**.
2. **Retournez le stylet** (extrémité gomme) et passez sur un dessin : il doit s'effacer, quel que soit l'outil actif. « Extrémité gomme détectée » → OK.
3. Appuyez sur le **bouton latéral** (« Bouton latéral détecté ») et inclinez le stylet (« Inclinaison détectée ») : informatifs.
4. Cliquez **Copier le rapport** et collez-le dans un message : il contient les verdicts et les mesures (nombre de niveaux de pression, etc.).

## Lecture des résultats

| Observation | Signification |
|---|---|
| « Pression reçue par Windows » OK mais « par l'application (egui) » NON | Windows voit la pression mais egui ne la transmet pas : l'épaisseur variable ne marchera pas ; la correction consiste à lire la pression directement depuis `inkboard-pen` (déjà disponible dans `PenSample`). |
| « Pression variable » NON avec peu de niveaux | Le pilote renvoie une pression binaire (0/1) ou la souris émule le stylet : vérifier le pilote / Windows Ink. |
| « Extrémité gomme » NON alors que l'extrémité est utilisée | Le pilote ne renseigne pas `PEN_FLAG_INVERTED` / `PEN_FLAG_ERASER` ; certains stylets n'ont pas de gomme matérielle. |
| Dernier pointeur « souris » alors qu'on utilise le stylet | Windows Ink est désactivé pour l'application ou le pilote émule la souris. |

## Ce qui est vérifié sans matériel

- L'interprétation des indicateurs du stylet (`inkboard-pen`) et la construction des verdicts (`inkboard-app/src/pen_test.rs`).
- L'installation du *subclass* de fenêtre sur une fenêtre factice qui n'altère pas les messages.
- Le comportement de l'application quand la gomme est détectée (le geste efface quel que soit l'outil actif) et le rendu de l'épaisseur selon la pression.
