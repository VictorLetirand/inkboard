//! Fichiers écrits comme le font les vrais outils (champs et réglages typiques d'Excalidraw et d'Obsidian Canvas),
//! pour vérifier que l'import tolère les champs supplémentaires, les éléments supprimés et les liaisons.

use inkboard_core::{ApproxMeasurer, Kind};
use inkboard_formats::import_any;

const EXCALIDRAW: &str = r##"{
  "type": "excalidraw",
  "version": 2,
  "source": "https://excalidraw.com",
  "elements": [
    {"id":"rect1","type":"rectangle","x":100,"y":100,"width":200,"height":100,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"#a5d8ff",
     "fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":{"type":3},
     "seed":123,"version":5,"versionNonce":99,"isDeleted":false,"boundElements":[{"id":"txt1","type":"text"},{"id":"arrow1","type":"arrow"}],
     "updated":1700000000000,"link":null,"locked":false},
    {"id":"txt1","type":"text","x":130,"y":135,"width":140,"height":25,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent",
     "fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,
     "seed":1,"version":3,"versionNonce":2,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,
     "text":"Bonjour","fontSize":20,"fontFamily":1,"textAlign":"center","verticalAlign":"middle","containerId":"rect1","originalText":"Bonjour","lineHeight":1.25},
    {"id":"ell1","type":"ellipse","x":500,"y":100,"width":120,"height":100,"angle":0.3,"strokeColor":"#e03131","backgroundColor":"transparent",
     "fillStyle":"hachure","strokeWidth":1,"strokeStyle":"dashed","roughness":2,"opacity":80,"groupIds":["g1"],"frameId":null,"roundness":null,
     "seed":2,"version":1,"versionNonce":3,"isDeleted":false,"boundElements":[{"id":"arrow1","type":"arrow"}],"updated":1,"link":null,"locked":false},
    {"id":"arrow1","type":"arrow","x":300,"y":150,"width":200,"height":0,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent",
     "fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":{"type":2},
     "seed":3,"version":1,"versionNonce":4,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,
     "points":[[0,0],[100,10],[200,0]],"lastCommittedPoint":null,
     "startBinding":{"elementId":"rect1","focus":0.0,"gap":1},"endBinding":{"elementId":"ell1","focus":0.0,"gap":1},
     "startArrowhead":null,"endArrowhead":"arrow","elbowed":false},
    {"id":"gone","type":"rectangle","x":0,"y":0,"width":50,"height":50,"angle":0,"strokeColor":"#000000","backgroundColor":"transparent",
     "fillStyle":"solid","strokeWidth":1,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,
     "seed":4,"version":1,"versionNonce":5,"isDeleted":true,"boundElements":null,"updated":1,"link":null,"locked":false},
    {"id":"free1","type":"freedraw","x":100,"y":300,"width":50,"height":20,"angle":0,"strokeColor":"#2f9e44","backgroundColor":"transparent",
     "fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,
     "seed":5,"version":1,"versionNonce":6,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,
     "points":[[0,0],[10,5],[25,10],[50,20]],"pressures":[0.2,0.5,0.7,0.4],"simulatePressure":false,"lastCommittedPoint":[50,20]},
    {"id":"free_text","type":"text","x":300,"y":300,"width":80,"height":25,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent",
     "fillStyle":"solid","strokeWidth":1,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,
     "seed":6,"version":1,"versionNonce":7,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,
     "text":"Note libre","fontSize":16,"fontFamily":2,"textAlign":"left","verticalAlign":"top","containerId":null,"originalText":"Note libre","lineHeight":1.25}
  ],
  "appState": {"gridSize":null,"viewBackgroundColor":"#ffffff"},
  "files": {}
}"##;

const OBSIDIAN_CANVAS: &str = r##"{
  "nodes": [
    {"id":"a1b2c3d4e5f60001","type":"text","text":"# Idée\n\nUn **texte** markdown","x":-200,"y":-100,"width":260,"height":140,"color":"4"},
    {"id":"a1b2c3d4e5f60002","type":"file","file":"Notes/Projet.md","x":200,"y":-100,"width":300,"height":200},
    {"id":"a1b2c3d4e5f60003","type":"link","url":"https://example.com","x":200,"y":200,"width":300,"height":120},
    {"id":"a1b2c3d4e5f60004","type":"group","label":"Groupe","x":-260,"y":-160,"width":820,"height":520}
  ],
  "edges": [
    {"id":"e1","fromNode":"a1b2c3d4e5f60001","fromSide":"right","toNode":"a1b2c3d4e5f60002","toSide":"left","toEnd":"arrow","label":"vers","color":"1"},
    {"id":"e2","fromNode":"a1b2c3d4e5f60002","fromSide":"bottom","toNode":"a1b2c3d4e5f60003","toSide":"top","fromEnd":"none"},
    {"id":"e3","fromNode":"a1b2c3d4e5f60002","toNode":"inconnu"}
  ]
}"##;

#[test]
fn imports_a_realistic_excalidraw_scene() {
    let imp = import_any("scene.excalidraw", EXCALIDRAW, &ApproxMeasurer).expect("import");
    let page = &imp.doc.pages[0];
    // l'élément supprimé est ignoré ; le texte lié devient l'étiquette de la forme
    let boxes: Vec<_> = page.objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b) } else { None }).collect();
    assert!(boxes.iter().any(|b| b.label.text.to_plain() == "Bonjour"), "texte lié à la forme");
    assert!(boxes.iter().any(|b| b.label.text.to_plain() == "Note libre"), "texte libre");
    assert_eq!(page.objects.iter().filter(|o| matches!(o.kind, Kind::Box(_))).count(), 3, "rect, ellipse, texte libre (pas l'élément supprimé)");
    // la flèche reste liée aux deux formes
    let conn = page.objects.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).expect("flèche");
    assert_eq!(conn.targets().count(), 2, "flèche liée aux deux extrémités");
    assert!(page.objects.iter().any(|o| matches!(o.kind, Kind::Pen(_))), "trait à main levée");
}

#[test]
fn excalidraw_import_survives_a_roundtrip_through_our_export() {
    let imp = import_any("scene.excalidraw", EXCALIDRAW, &ApproxMeasurer).unwrap();
    let out = inkboard_formats::excalidraw::export(&imp.doc, &imp.doc.pages[0], &ApproxMeasurer);
    let again = import_any("b.excalidraw", &out, &ApproxMeasurer).unwrap();
    assert_eq!(again.doc.pages[0].objects.len(), imp.doc.pages[0].objects.len());
}

#[test]
fn imports_an_obsidian_style_canvas() {
    let imp = import_any("Plan.canvas", OBSIDIAN_CANVAS, &ApproxMeasurer).expect("import");
    let page = &imp.doc.pages[0];
    let texts: Vec<String> = page.objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b.label.text.to_plain()) } else { None }).collect();
    assert!(texts.iter().any(|t| t.contains("Idée")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Projet.md")), "le nœud fichier montre son chemin : {texts:?}");
    assert!(texts.iter().any(|t| t.contains("example.com")), "le nœud lien montre l'URL : {texts:?}");
    let links = page.objects.iter().filter(|o| matches!(o.kind, Kind::Connector(_))).count();
    assert_eq!(links, 2, "l'arête vers un nœud inconnu est ignorée");
}
