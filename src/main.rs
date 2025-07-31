use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process;

use chrono::DateTime;
use memchr::memmem;
use serde::ser::{self, SerializeSeq};
use serde::{Deserialize, Serialize, Serializer};

fn main() -> Result<(), Box<dyn Error>> {
    let file = env::args().nth(1).ok_or(ArgError {})?;
    let claims: Vec<String> = env::args()
        .nth(2)
        .ok_or(ArgError {})?
        .split(",")
        .map(String::from)
        .collect();
    let natures: Vec<String> = env::args()
        .nth(3)
        .unwrap_or("".to_string())
        .split(",")
        .skip_while(|s| s.is_empty())
        .map(String::from)
        .collect();
    let mut conn = rusqlite::Connection::open("out.db")?;
    conn.execute("PRAGMA synchronous = off;", ())?; // YOLO, we need speed
    create_db_tables(&mut conn)?;
    let mut statements = Statements::new(&conn);
    fill_db_from_dump(file, claims, natures, &mut statements)?;
    generate_geojson(&mut statements)?;
    Ok(())
}
fn create_db_tables(conn: &mut rusqlite::Connection) -> Result<(), Box<dyn Error>> {
    conn.execute(
        "CREATE TABLE entities (
            id TEXT primary key,
            name_en TEXT,
            name_fr TEXT
        );",
        (),
    )?;
    conn.execute(
        "CREATE TABLE positions (
            id TEXT primary key,
            lat TEXT,
            lon TEXT,
            FOREIGN KEY(id) REFERENCES entities(id) ON DELETE CASCADE ON UPDATE CASCADE
        );",
        (),
    )?;
    conn.execute(
        "CREATE TABLE natures (
            id TEXT,
            nat TEXT,
            FOREIGN KEY(id) REFERENCES entities(id) ON DELETE CASCADE ON UPDATE CASCADE
        );
        ",
        (),
    )?;
    conn.execute("CREATE INDEX natures_id_nat ON natures(id, nat);", ())?;
    conn.execute(
        "CREATE TABLE edges (
            a TEXT not null,
            b TEXT not null,
            UNIQUE(a, b)
        );",
        (),
    )?;

    conn.execute("CREATE INDEX edges_a ON edges(a);", ())?;
    conn.execute("CREATE INDEX edges_b ON edges(b);", ())?;
    Ok(())
}
fn fill_db_from_dump(
    file: String,
    claims: Vec<String>,
    natures: Vec<String>,
    statements: &mut Statements,
) -> Result<(), Box<dyn Error>> {
    let mut cat = lbzcat(&file)?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .enumerate()
            // Skip empty first and last line
            .filter(|(_, line)| line.len() > 2)
            // cheap filter for faster processing; grepping multiple claims is much faster than
            // json parsing, and does faster elimination of non-matching content
            .filter(|(_, line)| claims.iter().all(|claim| grep(line, claim)))
            .for_each(|(_i, l)| {
                let el = parse(&l);
                if query(&el, &claims, &natures) {
                    //println!("{_i}: {}", _format(&el));
                    insert_base(statements, &el);
                    insert(statements, &el);
                }
            });
    }

    let res = cat.wait().map_err(|e| format!("Could not wait: {e}"))?;
    res.success().then_some(()).ok_or("failure")?;
    Ok(())
}

fn generate_geojson(statements: &mut Statements) -> Result<(), Box<dyn Error>> {
    // Get top 200 categories, and fetch their name
    //
    // Block those generic (too broad) categories that can be in many separate places:
    let banned_generic_categories = HashSet::from([
        "Q79007",   // street
        "Q3257686", // locality
        "Q188509",  // suburb
        "Q7543083", // avenue
        "Q3957",    // town
        "Q207934",  // avenue
        "Q123705",  // neighborhood
        "Q532",     // village
        "Q34442",   // road
        "Q54114",   // boulevard
        "Q1549591", // big city
        "Q486972",  // human settlement
        "Q5004679", // path
        "Q902814",  // border city
        "Q2983893", // quarter
        "Q515",     // city
        "Q41176",   // building
        "Q194203",  // arrondissement of France
        "Q2198484", // municipal district // accross Canada, Ireland, and Russia
        "Q3840711", // riverfront
        "Q703941",  // private road
        "Q82794",   // region
    ]);
    let top200 = &mut statements.top_200_categories_by_edges;
    let rows = top200.query_map([], |row| row.get(0))?;
    let mut categories = HashMap::new();
    for x in rows {
        let id: String = x?;
        if !banned_generic_categories.contains(&id.as_str()) {
            // Make sure we have the description of this category.
            let labels = fetch_missing_entity_name(
                &mut statements.select_entity,
                &mut statements.insert_entity,
                &id,
            )?;
            categories.insert(id, labels);
        }
    }

    std::fs::create_dir_all("web/geojson")?;
    let idx = File::create_new("web/geojson/index.json")?;
    serde_json::to_writer(idx, &categories)?;

    let select_nodes = &mut statements.select_entities_category;
    let select_links = &mut statements.select_edges_category;
    for id in categories.keys() {
        let nodes = File::create_new(format!("web/geojson/{id}-nodes.geojson"))?;
        let entities = select_nodes.query((id,))?;
        let geo = GeoJsonRootNodes::new(RefCell::new(entities));
        serde_json::to_writer(nodes, &geo)?;
        let links = File::create_new(format!("web/geojson/{id}-links.geojson"))?;
        let edges = select_links.query((id,))?;
        let geo = GeoJsonRootEdges::new(edges);
        serde_json::to_writer(links, &geo)?;
    }
    Ok(())
}

struct ArgError {}
impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "not enough arguments. Usage:\nborder-explorer <wikidata bz2 json file> <comma separated mandatory claims (AND)> [comma separated possible natures (OR)]"
        )
    }
}
impl std::fmt::Debug for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl Error for ArgError {}

fn lbzcat(file: &str) -> Result<process::Child, String> {
    let cat = process::Command::new("lbzcat")
        .arg(file)
        .stdout(process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch lbzcat: {e}"))?;

    Ok(cat)
}

fn grep(line: &str, needle: &str) -> bool {
    memmem::find(line.as_ref(), needle.as_ref()).is_some()
}

type Labels<'a> = HashMap<&'a str, Label<'a>>;
type LabelsQuery<'a> = HashMap<&'a str, Cow<'a, str>>;

#[derive(Deserialize)]
struct Element<'a> {
    #[serde(borrow)]
    claims: HashMap<&'a str, Vec<Claim<'a>>>,
    id: &'a str,
    labels: Labels<'a>,
}
#[derive(Deserialize, Debug)]
struct Claim<'a> {
    #[serde(borrow)]
    mainsnak: Snak<'a>,
    qualifiers: Option<HashMap<&'a str, Vec<Snak<'a>>>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "datatype", content = "datavalue")]
#[non_exhaustive]
enum Snak<'a> {
    #[serde(rename = "wikibase-item")]
    Item {
        #[serde(borrow)]
        value: Value<'a>,
    },
    #[serde(rename = "globe-coordinate")]
    GlobeCoordinate { value: Coord },
    #[serde(rename = "time")]
    Time { value: Time<'a> },

    // TODO: to remove (unused)
    /*
    #[serde(rename = "string")]
    #[serde(alias = "url")]
    Str { value: Cow<'a, str> },
    #[serde(rename = "external-id")]
    ExternalId(serde_json::Value),
    commonsMedia(serde_json::Value),
    quantity(serde_json::Value),
    monolingualtext(serde_json::Value),
    */
    // The rest
    #[allow(unused)]
    #[serde(untagged)]
    Unknown(serde_json::Value),
}
#[derive(Debug, Deserialize)]
struct Time<'a> {
    time: &'a str,
}
/*
struct Mainsnak<'a> {
    #[serde(borrow)]
    datavalue: Datavalue<'a>,
    datatype: Datatype,
}

#[derive(Deserialize)]
#[non_exhaustive]
enum Datatype {
    #[serde(rename = "wikibase-entityid")]
    WikibaseEntityId,
    String,
    GlobeCoordinate,
}
#[derive(Deserialize)]
struct Datavalue<'a> {
    value: serde_json::Value,
}
*/
#[derive(Debug, Deserialize)]
struct Value<'a> {
    id: &'a str,
}
#[derive(Debug, Deserialize)]
struct Coord {
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Deserialize)]
struct Label<'a> {
    //language: &'a str,
    value: Cow<'a, str>,
}
fn parse<'a>(l: &'a str) -> Element<'a> {
    //println!("line: {l}");
    let el: Element = serde_json::from_str(&l[0..(l.len() - 1)]).expect("not json");
    el
}
fn query<'a>(el: &Element<'a>, claims: &[String], nature_ids: &[String]) -> bool {
    //println!("{l}");
    /* Check that all claims we expect are indeed present */
    if !claims
        .iter()
        .all(|claim| el.claims.contains_key(claim.as_str()))
    {
        return false;
    }

    if nature_ids.is_empty() {
        return true;
    }

    if let Some(nature) = el.claims.get("P31") {
        if nature.iter().any(|nat| {
            //print!(".");
            nature_ids.iter().any(|possible_nature| {
                if let Snak::Item { value } = &nat.mainsnak {
                    value.id == possible_nature && claim_still_valid(nat)
                } else {
                    false
                }
            })
        }) {
            return true;
        }
    }
    false
}

fn claim_still_valid(claim: &Claim) -> bool {
    // check qualifier P582 (expiry date) of this claim
    if let Some(ref qualifiers) = claim.qualifiers {
        if let Some(expiries) = qualifiers.get("P582") {
            // Is it expired ? fixed date
            if claim_before(
                expiries,
                DateTime::parse_from_rfc3339("2025-01-01T00:00:00+00:00").expect("Cannot fail"),
            ) {
                return false;
            }
        }
    }
    true
}

fn claim_before<Tz: chrono::TimeZone>(p582_qualifiers: &[Snak], cutoff: DateTime<Tz>) -> bool {
    p582_qualifiers.iter().all(|expiry| {
        if let Snak::Time { value } = expiry {
            //println!("{claim:?}: '{}'", value.time);

            match DateTime::parse_from_str(value.time, "%+") {
                //DateTime::parse_from_str(value.time, "%Y-%m-%dT%H:%M:%SZ").unwrap()
                Ok(dt) => dt < cutoff,
                Err(e) => {
                    println!("Cannot parse date '{}': {e}", value.time);
                    /* Unparseable date, assume it's probably too old (year-only), and therefore
                     * the before the date we target*/
                    true
                }
            }
        } else {
            false
        }
    })
}

fn _format<'a>(item: &Element<'a>) -> String {
    format!(
        "{} ({}): {}",
        item.id,
        item.claims
            .get("P31")
            .unwrap_or(&vec![])
            .iter()
            .map(|nat| if let Snak::Item { value } = &nat.mainsnak {
                format!(
                    "{}{}",
                    value.id,
                    if !claim_still_valid(nat) {
                        "(obsolete)"
                    } else {
                        ""
                    }
                )
            } else {
                "<Unknown-Nature>".to_string()
            })
            .collect::<Vec<_>>()
            .join(", "),
        item.labels
            .get("en")
            .unwrap_or(&Label {
                //language: "x",
                value: Cow::from("<No-French-Label>")
            })
            .value
    )
}

#[expect(unused)]
fn count<'a>(natures: &mut HashMap<String, u64>, item: &Element<'a>) {
    item.claims
        .get("P31")
        .unwrap_or(&vec![])
        .iter()
        .for_each(|nat| {
            if let Snak::Item { value } = &nat.mainsnak {
                if claim_still_valid(nat) {
                    let nat = value.id.to_string();
                    (*natures.entry(nat).or_insert(0)) += 1;
                }
            }
        });
}

struct Statements<'conn> {
    insert_entity: rusqlite::Statement<'conn>,
    insert_position: rusqlite::Statement<'conn>,
    insert_nature: rusqlite::Statement<'conn>,
    insert_edge: rusqlite::Statement<'conn>,
    select_entity: rusqlite::Statement<'conn>,
    select_entities_category: rusqlite::Statement<'conn>,
    select_edges_category: rusqlite::Statement<'conn>,
    top_200_categories_by_edges: rusqlite::Statement<'conn>,
}
impl<'conn> Statements<'conn> {
    fn new(conn: &'conn rusqlite::Connection) -> Self {
        Self {
            insert_entity: conn
                .prepare(
                    "INSERT INTO entities (id, name_en, name_fr)
                        VALUES (?1, ?2, ?3);",
                )
                .expect("Failed to prepare insert entity"),
            insert_position: conn
                .prepare(
                    "INSERT INTO positions (id, lat, lon)
                        VALUES (?1, ?2, ?3);",
                )
                .expect("Failed to prepare insert position"),
            insert_nature: conn
                .prepare(
                    "INSERT INTO natures (id, nat)
                        VALUES (?1, ?2);",
                )
                .expect("Failed to prepare insert nature"),
            insert_edge: conn
                .prepare(
                    "INSERT OR IGNORE INTO edges (a, b)
                        VALUES (?1, ?2);",
                )
                .expect("Failed to prepare insert edge"),
            select_entity: conn
                .prepare("SELECT name_en, name_fr FROM entities WHERE id = ?1;")
                .expect("Failed to prepare select entity"),
            select_entities_category: conn
                .prepare("SELECT e.name_en, e.name_fr, p.lon, p.lat FROM entities as e, natures as n, positions as p WHERE n.nat = ?1 and n.id = e.id and p.id = e.id;")
                .expect("Failed to prepare select category"),
            select_edges_category: conn
                .prepare("SELECT a.lon, a.lat, b.lon, b.lat FROM edges as edj, natures as nat, entities as ent, positions as a, positions as b WHERE nat.nat = ?1 and ent.id = nat.id and edj.a = ent.id and nat.nat IN (SELECT nat from natures where id = edj.b) and edj.a = a.id and edj.b = b.id;")
                .expect("Failed to prepare select category"),
            top_200_categories_by_edges: conn
                .prepare("SELECT nat.nat, COUNT(edj.rowid) as c FROM edges as edj, natures as nat, entities as ent WHERE ent.id = nat.id and edj.a = ent.id and nat.nat IN (SELECT nat from natures where id = edj.b) GROUP BY nat.nat ORDER BY c DESC LIMIT 200;")
                .expect("Failed to prepare top 200 categories"),
        }
    }
}
fn insert_base<'a>(st: &mut Statements, item: &Element<'a>) {
    let label_en = label(&item.labels, "en").unwrap_or_else(|| label_or_empty(&item.labels, "mul"));
    let label_fr = label_or_empty(&item.labels, "fr");

    st.insert_entity
        .execute((item.id, label_en, label_fr))
        .expect("Failed base insert");
}
fn insert<'a>(st: &mut Statements, item: &Element<'a>) {
    let natures = item
        .claims
        .get("P31")
        .unwrap_or_else(|| {
            panic!("No nature for {}", item.id);
        })
        .iter()
        .filter(|nat| claim_still_valid(nat))
        .map(|nat| {
            if let Snak::Item { value } = &nat.mainsnak {
                value.id
            } else {
                panic!("No nature id")
            }
        });
    // We should not reach this code without an existing position
    let position = item
        .claims
        .get("P625")
        .expect("Should have a position")
        .iter()
        .filter_map(|pos| {
            if let Snak::GlobeCoordinate { ref value } = pos.mainsnak {
                Some(value)
            } else {
                None
            }
        })
        .next();
    // Ignore item with no position
    let position = match position {
        Some(pos) => pos,
        None => return,
    };
    let connections = item
        .claims
        .get("P47")
        .expect("Should have connections")
        .iter()
        .filter_map(|pos| {
            //dbg!(&pos.mainsnak);
            if let Snak::Item { ref value } = pos.mainsnak {
                Some(value.id)
            } else {
                None // Ignore elements explicitly without any item to share border with, like Q71356
            }
        });
    st.insert_position
        .execute((item.id, position.latitude, position.longitude))
        .expect("Failed insert");
    natures.for_each(|nat| {
        st.insert_nature
            .execute((item.id, nat))
            .expect("Failed nature insert");
    });
    connections.for_each(|edge| {
        let mut items = [item.id, edge];
        items.sort();
        st.insert_edge
            .execute((items[0], items[1]))
            .expect("Failed edge insert");
    });
}

fn label<'a>(labels: &Labels<'a>, lang: &str) -> Option<String> {
    labels.get(lang).map(|l| l.value.to_string())
}
fn label_or_empty<'a>(labels: &Labels<'a>, lang: &str) -> String {
    label(labels, lang).unwrap_or_default()
}

fn label_q<'a>(labels: &LabelsQuery<'a>, lang: &str) -> Option<String> {
    labels.get(lang).map(|l| l.to_string())
}
fn label_or_empty_q<'a>(labels: &LabelsQuery<'a>, lang: &str) -> String {
    label_q(labels, lang).unwrap_or_default()
}

fn fetch_missing_entity_name<'st>(
    select_entity: &mut rusqlite::Statement<'st>,
    insert_entity: &mut rusqlite::Statement<'st>,
    id: &str,
) -> Result<HashMap<&'static str, String>, Box<dyn Error>> {
    match select_entity.query_one((id,), |row| Ok((row.get(0), row.get(1)))) {
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Ok((en, fr)) => {
            return Ok(HashMap::from([("en", en?), ("fr", fr?)]));
        }
        Err(e) => return Err(format!("Cannot fetch: {e}").into()),
    };
    // Category is not present - fetch its name responsibly from the wikidata API, and cache the
    // result
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("border-explorer v", env!("CARGO_PKG_VERSION")))
        .cookie_store(true)
        .build()?
        .get(format!(
            "https://www.wikidata.org/w/rest.php/wikibase/v1/entities/items/{id}/labels"
        ))
        .send()?
        .bytes()?;
    let names: LabelsQuery = serde_json::from_slice(&resp)?;
    let label_en = label_q(&names, "en").unwrap_or_else(|| label_or_empty_q(&names, "mul"));
    let label_fr = label_or_empty_q(&names, "fr");
    insert_entity.execute((id, &label_en, &label_fr))?;
    Ok(HashMap::from([("en", label_en), ("fr", label_fr)]))
}

#[derive(Serialize)]
struct GeoJsonNode {
    #[serde(rename = "type")]
    typ: &'static str,
    properties: GeoJsonNodeProp,
    geometry: GeoJsonNodeGeo,
}
impl GeoJsonNode {
    fn new(en: String, fr: String, coordinates: [f64; 2]) -> Self {
        Self {
            typ: "Feature",
            properties: GeoJsonNodeProp { en, fr },
            geometry: GeoJsonNodeGeo {
                typ: "Point",
                coordinates,
            },
        }
    }
}
impl<'st> TryFrom<&rusqlite::Row<'st>> for GeoJsonNode {
    type Error = Box<dyn Error>;

    fn try_from(value: &rusqlite::Row<'st>) -> Result<Self, Self::Error> {
        let name_en: String = value.get(0)?;
        let name_fr: String = value.get(1)?;
        let lon: String = value.get(2)?;
        let lat: String = value.get(3)?;
        Ok(GeoJsonNode::new(
            name_en,
            name_fr,
            [
                lon.parse()
                    .map_err(|e| format!("failed to parse float {lon}: {e}"))?,
                lat.parse()
                    .map_err(|e| format!("failed to parse float {lat}: {e}"))?,
            ],
        ))
    }
}

#[derive(Serialize)]
struct GeoJsonNodeProp {
    en: String,
    fr: String,
}
#[derive(Serialize)]
struct GeoJsonNodeGeo {
    #[serde(rename = "type")]
    typ: &'static str,
    coordinates: [f64; 2],
}

#[derive(Serialize)]
struct GeoJsonRootNodes<'a> {
    #[serde(rename = "type")]
    typ: &'static str,
    features: RowsNode<'a>,
}
impl<'a> GeoJsonRootNodes<'a> {
    fn new(r: RefCell<rusqlite::Rows<'a>>) -> Self {
        Self {
            typ: "FeatureCollection",
            features: RowsNode { r },
        }
    }
}

struct RowsNode<'a> {
    r: RefCell<rusqlite::Rows<'a>>,
}

impl<'a> Serialize for RowsNode<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        fn err_conv(e: rusqlite::Error) -> String {
            format!("got sql error {e}")
        }
        let mut seq = serializer.serialize_seq(None)?;
        while let Some(ent) = self
            .r
            .borrow_mut()
            .next()
            .map_err(|e| ser::Error::custom(err_conv(e)))?
        {
            let node = GeoJsonNode::try_from(ent).map_err(ser::Error::custom)?;
            seq.serialize_element(&node)?;
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct GeoJsonRootEdges<'a> {
    #[serde(rename = "type")]
    typ: &'static str,
    coordinates: RowsEdges<'a>,
}
impl<'a> GeoJsonRootEdges<'a> {
    fn new(rows: rusqlite::Rows<'a>) -> Self {
        Self {
            typ: "MultiLineString",
            coordinates: RowsEdges {
                r: RefCell::new(rows),
            },
        }
    }
}
struct RowsEdges<'a> {
    r: RefCell<rusqlite::Rows<'a>>,
}

struct LineCoord {
    coordinates: [[f64; 2]; 2],
}
impl<'st> TryFrom<&rusqlite::Row<'st>> for LineCoord {
    type Error = Box<dyn Error>;

    fn try_from(value: &rusqlite::Row<'st>) -> Result<Self, Self::Error> {
        let mut f: [f64; 4] = [999999.99; 4];
        for (i, coord) in f.iter_mut().enumerate() {
            let val: String = value.get(i)?;
            *coord = val
                .parse()
                .map_err(|e| format!("failed to parse float {val}: {e}"))?;
        }
        Ok(LineCoord {
            coordinates: [[f[0], f[1]], [f[2], f[3]]],
        })
    }
}

// Failed to make it generic, let's copy paste instead
impl<'a> Serialize for RowsEdges<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        fn err_conv(e: rusqlite::Error) -> String {
            format!("got sql error {e}")
        }
        let mut seq = serializer.serialize_seq(None)?;
        while let Some(ent) = self
            .r
            .borrow_mut()
            .next()
            .map_err(|e| ser::Error::custom(err_conv(e)))?
        {
            let node = LineCoord::try_from(ent).map_err(ser::Error::custom)?;
            seq.serialize_element(&node.coordinates)?;
        }
        seq.end()
    }
}
