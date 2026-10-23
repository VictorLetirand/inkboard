//! Mini XML DOM: enough to read draw.io files.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }
}

fn node_from(e: &BytesStart<'_>) -> Result<Node, String> {
    let mut node = Node { name: String::from_utf8_lossy(e.name().as_ref()).into_owned(), ..Node::default() };
    for a in e.attributes() {
        let a = a.map_err(|x| x.to_string())?;
        let k = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|x| x.to_string())?.into_owned();
        node.attrs.insert(k, v);
    }
    Ok(node)
}

/// Parses an XML document and returns its root element.
pub fn parse(src: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(src);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event().map_err(|e| format!("invalid XML: {e}"))? {
            Event::Start(e) => stack.push(node_from(&e)?),
            Event::Empty(e) => {
                let n = node_from(&e)?;
                match stack.last_mut() {
                    Some(p) => p.children.push(n),
                    None => root = Some(n),
                }
            }
            Event::End(_) => {
                let n = stack.pop().ok_or("balise fermante inattendue")?;
                match stack.last_mut() {
                    Some(p) => p.children.push(n),
                    None => root = Some(n),
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.decode().map_err(|e| e.to_string())?);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(top) = stack.last_mut() {
                    if let Some(c) = r.resolve_char_ref().map_err(|e| e.to_string())? {
                        top.text.push(c);
                    } else {
                        match r.decode().map_err(|e| e.to_string())?.as_ref() {
                            "lt" => top.text.push('<'),
                            "gt" => top.text.push('>'),
                            "amp" => top.text.push('&'),
                            "quot" => top.text.push('"'),
                            "apos" => top.text.push('\''),
                            _ => {}
                        }
                    }
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("incomplete XML document".into());
    }
    root.ok_or_else(|| "empty XML document".into())
}

/// Escaping for an attribute value.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_with_entities() {
        let n = parse(r#"<?xml version="1.0"?><a x="1 &amp; 2"><b y="&lt;z&gt;"/><c>text &amp; more</c></a>"#).unwrap();
        assert_eq!(n.name, "a");
        assert_eq!(n.attr("x"), Some("1 & 2"));
        assert_eq!(n.child("b").unwrap().attr("y"), Some("<z>"));
        assert_eq!(n.child("c").unwrap().text, "text & more");
        let mut v = Vec::new();
        n.find_all("b", &mut v);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("").is_err());
        assert!(parse("<a>").is_err());
    }
}
