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
    let needle = env::args().nth(2).ok_or(ArgError {})?;
    let mut cat = lbzcat(&file)?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .enumerate()
            .for_each(|(i, line)| {
                if let Some(l) = grep(&line, &needle) {
                    //println!("{i}");
                    if let Some(el) = query(l, "P47", &[] /*&["Q484170"]*/) {
                        println!("{i}: {}", format(el));
                    }
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
        write!(f, "not enough arguments")
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

fn grep<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    if memmem::find(line.as_ref(), needle.as_ref()).is_some() {
        return Some(line);
    }
    None
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
    #[serde(rename = "string")]
    #[serde(alias = "url")]
    Str {
        value: Cow<'a, str>,
    },
    #[serde(rename = "globe-coordinate")]
    GlobeCoordinate {
        value: Coord,
    },
    #[serde(rename = "time")]
    Time {
        value: Time<'a>,
    },

    // TODO: to remove (unused)
    #[serde(rename = "external-id")]
    ExternalId(serde_json::Value),
    commonsMedia(serde_json::Value),
    quantity(serde_json::Value),
    monolingualtext(serde_json::Value),
    // The rest
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
    language: &'a str,
    value: Cow<'a, str>,
}
fn query<'a>(l: &'a str, claim: &str, nature_ids: &[&str]) -> Option<Element<'a>> {
    //println!("{l}");
    let el: Element = serde_json::from_str(&l[0..(l.len() - 1)]).expect("not json");
    el.claims.contains_key(claim).then_some(())?;
    el.claims.contains_key("P625").then_some(())?;
    if nature_ids.is_empty() {
        return Some(el);
    }

    if let Some(nature) = el.claims.get("P31") {
        if nature.iter().any(|nat| {
            //print!(".");
            nature_ids.iter().any(|possible_nature| {
                if let Snak::Item { value } = &nat.mainsnak {
                    &value.id == possible_nature && claim_still_valid(nat)
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

struct Output {}
fn format<'a>(item: Element<'a>) -> String {
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
            .get("fr")
            .unwrap_or(&Label {
                language: "x",
                value: Cow::from("<No-French-Label>")
            })
            .value
    )
}
