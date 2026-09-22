//! The `tiny_http` server: binds `0.0.0.0:8790` (`UNDERCROFT_EDITOR_ADDR`), data dir
//! `GameData::workspace_data_dir()` (`UNDERCROFT_DATA_DIR`), fixtures dir `GameData::workspace_fixtures_dir()`
//! (`UNDERCROFT_FIXTURES_DIR`), `DESIGN.md` next to `assets/` (`UNDERCROFT_DESIGN_MD`). Routes: `GET /` (the embedded `web/index.html`), `GET /api/zones`,
//! `POST /api/preview`, `POST /api/save`; every request reloads `GameData` from disk, JSON responses set
//! `Content-Type: application/json`, errors are `{ "error": "…" }` with a 4xx/5xx status.

use crate::doc::{self, ZoneDoc};
use crate::{design_md, save};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tiny_http::{Header, Method, Request, Response, Server};
use undercroft_data::zone::CreatureSpawn;
use undercroft_data::{GameData, ZoneDef};
use undercroft_sim::validate::Validation;

const INDEX_HTML: &str = include_str!("../web/index.html");

/// Where the server reads and writes.
pub struct Config {
    pub addr: String,
    pub data_dir: PathBuf,
    pub fixtures_dir: PathBuf,
    /// The DESIGN.md whose ASCII map blocks a save rewrites; skipped when it does not exist.
    pub design_md: PathBuf,
}

impl Config {
    /// From the environment (`UNDERCROFT_EDITOR_ADDR`, `UNDERCROFT_DATA_DIR`, `UNDERCROFT_FIXTURES_DIR`,
    /// `UNDERCROFT_DESIGN_MD`; the last defaults to `<data dir>/../../DESIGN.md`).
    pub fn from_env() -> Config {
        let dir = |key: &str, default: PathBuf| {
            std::env::var_os(key).map(PathBuf::from).unwrap_or(default)
        };
        let data_dir = dir("UNDERCROFT_DATA_DIR", GameData::workspace_data_dir());
        Config {
            addr: std::env::var("UNDERCROFT_EDITOR_ADDR").unwrap_or_else(|_| "0.0.0.0:8790".into()),
            design_md: dir("UNDERCROFT_DESIGN_MD", design_md::default_path(&data_dir)),
            data_dir,
            fixtures_dir: dir(
                "UNDERCROFT_FIXTURES_DIR",
                GameData::workspace_fixtures_dir(),
            ),
        }
    }
}

/// `GET /api/zones` item.
#[derive(Debug, Clone, Serialize)]
pub struct ZoneInfo {
    pub doc: ZoneDoc,
    pub name: String,
    pub size: i32,
    pub entry: undercroft_data::EntryKind,
    pub hunters: Vec<String>,
    pub creatures: Vec<CreatureSpawn>,
    pub palette: PaletteOut,
    pub kinds: Vec<u8>,
    pub validation: Validation,
    /// `doc::fingerprint` of the zone as loaded; send it back with `/api/save` as `base`.
    pub base: String,
}

/// The subset of the palette the page draws with (`0xRRGGBB` ints).
#[derive(Debug, Clone, Serialize)]
pub struct PaletteOut {
    pub floor: u32,
    pub wall: u32,
    pub pillar: u32,
    pub deep: u32,
    pub water: u32,
    pub ceil: u32,
}

#[derive(Serialize)]
struct ZonesOut {
    zones: Vec<ZoneInfo>,
}

#[derive(Deserialize)]
struct DocBody {
    doc: ZoneDoc,
    /// The fingerprint the page loaded the zone with (`ZoneInfo::base`); a save with a stale one is refused.
    #[serde(default)]
    base: Option<String>,
}

/// `/api/preview` reply: the preview plus the zone's current on-disk fingerprint so the page can notice a
/// hand edit while it is still editing.
#[derive(Serialize)]
struct PreviewOut {
    #[serde(flatten)]
    preview: doc::PreviewResult,
    base: String,
}

#[derive(Serialize)]
struct ErrorOut {
    error: String,
}

/// A response body plus status: the handlers are pure so they can be tested without a socket.
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Reply {
    fn json<T: Serialize>(status: u16, value: &T) -> Reply {
        match serde_json::to_string(value) {
            Ok(body) => Reply {
                status,
                content_type: "application/json",
                body,
            },
            Err(e) => Reply::error(500, format!("could not serialise the response: {e}")),
        }
    }

    fn error(status: u16, error: String) -> Reply {
        Reply {
            status,
            content_type: "application/json",
            body: serde_json::to_string(&ErrorOut { error })
                .unwrap_or_else(|_| "{\"error\":\"could not serialise the error\"}".to_string()),
        }
    }
}

fn zone_info(data: &GameData, zone: &ZoneDef) -> Result<ZoneInfo, String> {
    let preview = doc::preview_applied(data, &zone.id);
    let kinds = preview
        .kinds
        .ok_or_else(|| preview.parse_error.clone().unwrap_or_default())?;
    let validation = preview
        .validation
        .ok_or_else(|| format!("{}: no validation", zone.id))?;
    let p = &zone.palette;
    Ok(ZoneInfo {
        doc: ZoneDoc::from_zone(zone),
        name: zone.name.clone(),
        size: zone.size,
        entry: zone.entry,
        hunters: zone.hunters.clone(),
        creatures: zone.creatures.clone(),
        palette: PaletteOut {
            floor: p.floor,
            wall: p.wall,
            pillar: p.pillar,
            deep: p.deep,
            water: p.water,
            ceil: p.ceil,
        },
        kinds,
        validation,
        base: doc::fingerprint(data, zone),
    })
}

fn load(data_dir: &Path) -> Result<GameData, Reply> {
    GameData::from_dir(data_dir)
        .map_err(|e| Reply::error(500, format!("the data directory does not load: {e}")))
}

fn parse_doc(body: &str) -> Result<DocBody, Reply> {
    serde_json::from_str::<DocBody>(body)
        .map_err(|e| Reply::error(400, format!("bad request body: {e}")))
}

/// Route one request. `body` is the request body (empty for GET).
pub fn handle(config: &Config, method: &Method, url: &str, body: &str) -> Reply {
    let path = url.split('?').next().unwrap_or("");
    match (method, path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: INDEX_HTML.to_string(),
        },
        (Method::Get, "/api/zones") => {
            let data = match load(&config.data_dir) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let mut zones = Vec::new();
            for z in &data.zones {
                match zone_info(&data, z) {
                    Ok(info) => zones.push(info),
                    Err(e) => return Reply::error(500, format!("{}: {e}", z.id)),
                }
            }
            Reply::json(200, &ZonesOut { zones })
        }
        (Method::Post, "/api/preview") => {
            let DocBody { doc, .. } = match parse_doc(body) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let data = match load(&config.data_dir) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let Some(zone) = data.zone(&doc.id) else {
                return Reply::error(404, format!("unknown zone {:?}", doc.id));
            };
            let out = PreviewOut {
                base: doc::fingerprint(&data, zone),
                preview: doc::preview(&data, &doc),
            };
            Reply::json(200, &out)
        }
        (Method::Post, "/api/save") => {
            let DocBody { doc, base } = match parse_doc(body) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let data = match load(&config.data_dir) {
                Ok(d) => d,
                Err(r) => return r,
            };
            if data.zone(&doc.id).is_none() {
                return Reply::error(404, format!("unknown zone {:?}", doc.id));
            }
            let result = save::save(
                &doc,
                base.as_deref(),
                &config.data_dir,
                &config.fixtures_dir,
                &config.design_md,
            );
            Reply::json(200, &result)
        }
        (Method::Get, _) | (Method::Post, _) => {
            Reply::error(404, format!("no route for {method} {path}"))
        }
        _ => Reply::error(405, format!("{method} is not supported")),
    }
}

fn respond(request: Request, reply: Reply) {
    let header = Header::from_bytes("Content-Type", reply.content_type)
        .expect("a static content type is a valid header");
    let response = Response::from_string(reply.body)
        .with_status_code(reply.status)
        .with_header(header);
    if let Err(e) = request.respond(response) {
        eprintln!("editor: could not send the response: {e}");
    }
}

/// Bind and serve forever (requests are handled one at a time).
pub fn run(config: &Config) -> Result<(), String> {
    let server = Server::http(&config.addr).map_err(|e| format!("bind {}: {e}", config.addr))?;
    println!("editor: http://{}/", config.addr);
    println!("data: {}", config.data_dir.display());
    println!("fixtures: {}", config.fixtures_dir.display());
    println!(
        "DESIGN.md: {}{}",
        config.design_md.display(),
        if config.design_md.is_file() {
            ""
        } else {
            " (not found: map blocks will not be rewritten)"
        }
    );
    for mut request in server.incoming_requests() {
        let mut body = String::new();
        if let Err(e) = request.as_reader().read_to_string(&mut body) {
            let method = request.method().clone();
            let url = request.url().to_string();
            println!("{method} {url} -> 400 (unreadable body: {e})");
            respond(request, Reply::error(400, format!("unreadable body: {e}")));
            continue;
        }
        let method = request.method().clone();
        let url = request.url().to_string();
        let reply = handle(config, &method, &url, &body);
        println!(
            "{method} {url} -> {} ({} bytes)",
            reply.status,
            reply.body.len()
        );
        respond(request, reply);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            addr: "127.0.0.1:0".into(),
            data_dir: GameData::workspace_data_dir(),
            fixtures_dir: GameData::workspace_fixtures_dir(),
            design_md: design_md::default_path(&GameData::workspace_data_dir()),
        }
    }

    #[test]
    fn routes_answer_with_the_contract_shapes() {
        let c = config();
        let r = handle(&c, &Method::Get, "/", "");
        assert_eq!(r.status, 200);
        assert!(r.content_type.starts_with("text/html"));

        let r = handle(&c, &Method::Get, "/api/zones", "");
        assert_eq!(r.status, 200);
        let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        let zones = v["zones"].as_array().unwrap();
        assert_eq!(zones.len(), 4);
        let u = &zones[0];
        assert_eq!(u["doc"]["id"], "undercroft");
        assert_eq!(u["doc"]["rows"].as_array().unwrap().len(), 62);
        assert_eq!(u["doc"]["anchors"]["entry"], serde_json::json!([31, 58]));
        assert_eq!(u["entry"], "S");
        assert_eq!(u["size"], 62);
        assert_eq!(u["kinds"].as_array().unwrap().len(), 62 * 62);
        assert_eq!(u["validation"]["ok"], true);
        assert!(u["palette"]["floor"].is_u64());
        assert!(u["hunters"].is_array() && u["creatures"].is_array());
        assert_eq!(u["base"].as_str().map(str::len), Some(16), "a fingerprint");

        let body = serde_json::json!({ "doc": u["doc"] }).to_string();
        let r = handle(&c, &Method::Post, "/api/preview", &body);
        assert_eq!(r.status, 200);
        let p: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert!(p["parse_error"].is_null());
        assert_eq!(p["validation"]["ok"], true);
        assert_eq!(p["validation"]["errors"].as_array().unwrap().len(), 0);
        assert_eq!(
            p["base"], u["base"],
            "the preview carries the on-disk fingerprint"
        );

        let mut short = u["doc"].clone();
        let row = short["rows"][4].as_str().unwrap().to_string();
        short["rows"][4] = serde_json::Value::String(row[..row.len() - 1].to_string());
        let body = serde_json::json!({ "doc": short }).to_string();
        let r = handle(&c, &Method::Post, "/api/preview", &body);
        assert_eq!(r.status, 200);
        let p: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert!(p["parse_error"].as_str().unwrap().contains("row 4"));
        assert!(p["kinds"].is_null() && p["validation"].is_null());

        let r = handle(&c, &Method::Post, "/api/preview", "{not json");
        assert_eq!(r.status, 400);
        let e: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert!(e["error"].is_string());
        assert_eq!(handle(&c, &Method::Get, "/nope", "").status, 404);
        assert_eq!(handle(&c, &Method::Delete, "/api/zones", "").status, 405);
        let mut other = u["doc"].clone();
        other["id"] = serde_json::Value::String("nowhere".into());
        let body = serde_json::json!({ "doc": other }).to_string();
        assert_eq!(handle(&c, &Method::Post, "/api/preview", &body).status, 404);
    }
}
