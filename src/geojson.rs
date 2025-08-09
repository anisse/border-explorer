use crate::db::Statements;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;

use serde::ser::{self, SerializeSeq};
use serde::{Serialize, Serializer};

pub(crate) fn generate(
    statements: &mut Statements,
    banned_generic_categories: &HashSet<u64>,
) -> Result<(), Box<dyn Error>> {
    // Get top N categories, and fetch their name
    let top = &mut statements.top_categories_by_edges;
    let rows = top.query_map([], |row| row.get(0))?;
    let mut categories = HashMap::new();
    for x in rows {
        let id: u64 = x?;
        if !banned_generic_categories.contains(&id) {
            // Make sure we have the description of this category.
            let labels = fetch_missing_entity_name(
                &mut statements.select_entity,
                &mut statements.insert_entity,
                id,
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

fn fetch_missing_entity_name<'st>(
    select_entity: &mut rusqlite::Statement<'st>,
    insert_entity: &mut rusqlite::Statement<'st>,
    id: u64,
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
            "https://www.wikidata.org/w/rest.php/wikibase/v1/entities/items/Q{id}/labels"
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
type LabelsQuery<'a> = HashMap<&'a str, Cow<'a, str>>;
fn label_q<'a>(labels: &LabelsQuery<'a>, lang: &str) -> Option<String> {
    labels.get(lang).map(|l| l.to_string())
}
fn label_or_empty_q<'a>(labels: &LabelsQuery<'a>, lang: &str) -> String {
    label_q(labels, lang).unwrap_or_default()
}
