mod db;
mod geojson;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::process;

use chrono::DateTime;
use memchr::memmem;
use serde::Deserialize;

fn main() -> Result<(), Box<dyn Error>> {
    let mut config = Config::default();
    let mut args = env::args().skip(1);
    if let Some(file) = args.next() {
        config.intermediate_db_filename = file;
    }
    if let Some(file) = args.next() {
        config.wikidata_dump_filename = Some(file);
    }
    if let Some(natures) = args.next() {
        config.filtered_natures = natures
            .split(",")
            .skip_while(|s| s.is_empty())
            .map(String::from)
            .collect();
    }
    let mut conn = rusqlite::Connection::open(&config.intermediate_db_filename)?;
    conn.execute("PRAGMA synchronous = off;", ())?; // YOLO, we need speed

    /* If no dump filename is passed, we consider that we already have an sqlite file to work with */
    if config.wikidata_dump_filename.is_some() {
        db::create_tables(&mut conn)?;
    }
    let mut statements = db::Statements::new(&conn);
    if config.wikidata_dump_filename.is_some() {
        fill_db_from_dump(&config, &mut statements)?;
    }
    geojson::generate(&mut statements, &config.banned_generic_categories)?;
    Ok(())
}
struct Config {
    // I initially envisionned a pipeline that would be heavily configurable. But this is at odds
    // with putting things in a fixed-schema SQL DB, otherwise we'd just be replicating the
    // original wikidata graph database
    //
    // The below config options are provided with hardcoded defaults. There is no plan to make them
    // any more configurable without modifying the code at this time
    wikidata_dump_filename: Option<String>,
    mandatory_claims: Vec<&'static str>,
    filtered_natures: Vec<String>,
    intermediate_db_filename: String,

    banned_generic_categories: HashSet<&'static str>,
}
pub(crate) const NATURE_CLAIM: &str = "P31";
pub(crate) const POSITION_CLAIM: &str = "P625";
pub(crate) const SHARES_BORDER_WITH_CLAIM: &str = "P47";
const EXPIRY_CLAIM: &str = "P582";

impl Default for Config {
    fn default() -> Self {
        Self {
            wikidata_dump_filename: None,
            mandatory_claims: vec![NATURE_CLAIM, POSITION_CLAIM, SHARES_BORDER_WITH_CLAIM],
            filtered_natures: vec![],
            intermediate_db_filename: "border-explorer.db".to_string(),

            // Block those generic (too broad) categories that can be in many separate places:
            banned_generic_categories: HashSet::from([
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
            ]),
        }
    }
}
fn fill_db_from_dump(
    config: &Config,
    statements: &mut db::Statements,
) -> Result<(), Box<dyn Error>> {
    let mut cat = lbzcat(
        config
            .wikidata_dump_filename
            .as_ref()
            .ok_or("missing dump file name")?,
    )?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .enumerate()
            // Skip empty first and last line
            .filter(|(_, line)| line.len() > 2)
            // cheap filter for faster processing; grepping multiple claims is much faster than
            // json parsing, and does faster elimination of non-matching content
            .filter(|(_, line)| {
                config
                    .mandatory_claims
                    .iter()
                    .all(|claim| grep(line, claim))
            })
            .for_each(|(_i, l)| {
                let el = parse(&l);
                if query(&el, config) {
                    //println!("{_i}: {}", _format(&el));
                    db::insert_base(statements, &el);
                    db::insert(statements, &el);
                }
            });
    }

    let res = cat.wait().map_err(|e| format!("Could not wait: {e}"))?;
    res.success().then_some(()).ok_or("failure")?;
    Ok(())
}

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

    // The rest
    #[allow(unused)]
    #[serde(untagged)]
    Unknown(serde_json::Value),
}
#[derive(Debug, Deserialize)]
struct Time<'a> {
    time: &'a str,
    precision: u8,
}
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
fn query<'a>(el: &Element<'a>, config: &Config) -> bool {
    //println!("{l}");
    /* Check that all claims we expect are indeed present */
    if !config
        .mandatory_claims
        .iter()
        .all(|claim| el.claims.contains_key(claim))
    {
        return false;
    }

    if config.filtered_natures.is_empty() {
        return true;
    }

    if let Some(nature) = el.claims.get(NATURE_CLAIM) {
        if nature.iter().any(|nat| {
            //print!(".");
            config.filtered_natures.iter().any(|possible_nature| {
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

pub(crate) fn claim_still_valid(claim: &Claim) -> bool {
    // check qualifier P582 (expiry date) of this claim
    if let Some(ref qualifiers) = claim.qualifiers {
        if let Some(expiries) = qualifiers.get(EXPIRY_CLAIM) {
            // Is it expired ? fixed date
            if claim_before(
                expiries,
                // TODO: use current year instead
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
            //println!("'{}' (precision {})", value.time, value.precision);

            let s = match value.precision {
                0..=9 => &format!("{}-01-01T00:00:00Z", &value.time[..5]),
                10 => &format!("{}-01T00:00:00Z", &value.time[..8]),
                _ => value.time,
            };
            let dt = match DateTime::parse_from_str(s, "%+") {
                Ok(dt) => dt.to_utc(),
                Err(e) => {
                    println!(
                        "Cannot parse date '{}' of precision {}: {e}",
                        s, value.precision
                    );
                    /* Unparseable date, assume it's probably too old (year-only), and therefore
                     * the before the date we target*/
                    return true;
                }
            };
            dt < cutoff
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
            .get(NATURE_CLAIM)
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
        .get(NATURE_CLAIM)
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
