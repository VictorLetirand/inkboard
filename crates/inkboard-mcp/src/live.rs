//! Mode direct : les outils sont transmis à l'application ouverte, qui les applique à son propre tableau (historique d'annulation, sélection et
//! vue conservés). L'application publie un serveur local (127.0.0.1) et un fichier de découverte contenant le port et un jeton secret.
//!
//! Protocole : une ligne JSON par requête `{"token", "tool", "args"}`, une ligne JSON par réponse `{"ok": true, "result"}`,
//! `{"ok": false, "error"}` ou `{"ok": false, "other_file": true}` (l'application affiche un autre fichier que `args.path`).

use crate::Server;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Dossier des données de l'application : `%APPDATA%\Inkboard` (Windows), `~/Library/Application Support/Inkboard` (macOS),
/// `$XDG_DATA_HOME/Inkboard` ou `~/.local/share/Inkboard` (Linux) ; le dossier courant en dernier recours.
pub fn data_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(".local").join("share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("Inkboard")
}

/// Fichier de découverte : `live.json` dans le dossier des données.
pub fn discovery_path() -> PathBuf {
    data_dir().join("live.json")
}

/// Port et jeton publiés par l'application.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub port: u16,
    pub token: String,
}

pub fn read_info(file: &Path) -> Option<Info> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some(Info { port: u16::try_from(v.get("port")?.as_u64()?).ok()?, token: v.get("token")?.as_str()?.to_owned() })
}

/// Résultat d'une tentative de transmission.
#[derive(Debug)]
pub enum Forward {
    /// L'application a traité la demande (succès ou erreur de l'outil).
    Done(Result<Value, String>),
    /// Pas d'application joignable, ou elle affiche un autre fichier : le mode fichier prend le relais.
    NotLive,
}

fn request(info: &Info, req: &Value) -> std::io::Result<Value> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], info.port)), Duration::from_millis(500))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    writeln!(s, "{req}")?;
    s.flush()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Transmet l'outil à l'application si elle est ouverte (et affiche le fichier `args.path`, s'il est donné).
pub fn forward(srv: &Server, tool: &str, args: &Value) -> Forward {
    if srv.no_live {
        return Forward::NotLive;
    }
    let file = srv.live_file.clone().unwrap_or_else(discovery_path);
    let Some(info) = read_info(&file) else { return Forward::NotLive };
    match request(&info, &json!({"token": info.token, "tool": tool, "args": args})) {
        Err(_) => Forward::NotLive, // application fermée, fichier de découverte périmé
        Ok(r) if r["other_file"] == json!(true) => Forward::NotLive,
        Ok(r) if r["ok"] == json!(true) => Forward::Done(Ok(r["result"].clone())),
        Ok(r) => Forward::Done(Err(r["error"].as_str().unwrap_or("erreur de l'application").to_owned())),
    }
}
