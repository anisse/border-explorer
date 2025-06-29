use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader};
use std::process;

use chrono::DateTime;
use memchr::memmem;
use serde::Deserialize;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let mut cat = lbzcat(&file)?;
    let conn = rusqlite::Connection::open("out.db")?;
    conn.execute("PRAGMA synchronous = off;", ())?; // YOLO, we need speed
    conn.execute(
        "CREATE TABLE items (
            id TEXT primary key,
            lat TEXT,
            lon TEXT,
            name_en TEXT,
            name_fr TEXT
        );",
        (),
    )?;
    conn.execute(
        "CREATE TABLE natures (
            id TEXT,
            nat TEXT,
            FOREIGN KEY(id) REFERENCES items(id) ON DELETE CASCADE ON UPDATE CASCADE
        );
        ",
        (),
    )?;
    conn.execute(
        "CREATE TABLE edges (
            a TEXT not null,
            b TEXT not null,
            UNIQUE(a, b)
        );",
        (),
    )?;

    conn.execute(
        "CREATE INDEX edges_a ON edges(a);
        ",
        (),
    )?;
    conn.execute(
        "CREATE INDEX edges_b ON edges(b);
        ",
        (),
    )?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .enumerate()
            // cheap filter for faster processing
            .filter(|(_, line)| claims.iter().all(|claim| grep(line, claim)))
            .for_each(|(i, l)| {
                if let Some(el) = query(&l, &claims, &natures) {
                    println!("{i}: {}", format(&el));
                    insert(&conn, &el);
                }
            });
    }

    let res = cat.wait().map_err(|e| format!("Could not wait: {e}"))?;
    res.success().then_some(()).ok_or("failure")?;
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
impl std::error::Error for ArgError {}

fn lbzcat(file: &str) -> Result<process::Child, String> {
    let cat = process::Command::new("lbzcat")
        .arg(file)
        .stdout(process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch lbzcat: {e}"))?;

    Ok(cat)
}

fn grep<'a>(line: &'a str, needle: &str) -> bool {
    memmem::find(line.as_ref(), needle.as_ref()).is_some()
}

#[derive(Deserialize)]
struct Element<'a> {
    #[serde(borrow)]
    claims: HashMap<&'a str, Vec<Claim<'a>>>,
    id: &'a str,
    labels: HashMap<&'a str, Label<'a>>,
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
fn query<'a>(l: &'a str, claims: &[String], nature_ids: &[String]) -> Option<Element<'a>> {
    //println!("{l}");
    let el: Element = serde_json::from_str(&l[0..(l.len() - 1)]).expect("not json");
    /* Check that all claims we expect are indeed present */
    claims
        .iter()
        .all(|claim| el.claims.contains_key(claim.as_str()))
        .then_some(())?;
    if nature_ids.is_empty() {
        return Some(el);
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
            return Some(el);
        }
    }
    None
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

fn format<'a>(item: &Element<'a>) -> String {
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

fn insert<'a>(conn: &rusqlite::Connection, item: &Element<'a>) {
    let label_en = label_or_empty(item, "en");
    let label_fr = label_or_empty(item, "fr");
    let natures = item
        .claims
        .get("P31")
        .unwrap_or_else(|| {
            panic!("No nature for {}: {} - {}", item.id, label_en, label_fr);
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
    conn.execute(
        "INSERT INTO items (id, lat, lon, name_en, name_fr)
            VALUES (?1, ?2, ?3, ?4, ?5);",
        (
            item.id,
            position.latitude,
            position.longitude,
            label_en,
            label_fr,
        ),
    )
    .expect("Failed insert");
    natures.for_each(|nat| {
        conn.execute(
            "INSERT INTO natures (id, nat)
                VALUES (?1, ?2);",
            (item.id, nat),
        )
        .expect("Failed nature insert");
    });
    connections.for_each(|edge| {
        let mut items = [item.id, edge];
        items.sort();
        conn.execute(
            "INSERT OR IGNORE INTO edges (a, b)
                VALUES (?1, ?2);",
            (items[0], items[1]),
        )
        .expect("Failed nature insert");
    });
}

fn label_or_empty<'a>(item: &Element<'a>, lang: &str) -> String {
    item.labels
        .get(lang)
        .unwrap_or(&Label {
            //language: "x",
            value: Cow::from(""),
        })
        .value
        .to_string()
}
