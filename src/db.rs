use super::Element;
use super::Labels;
use super::NATURE_CLAIM;
use super::POSITION_CLAIM;
use super::SHARES_BORDER_WITH_CLAIM;
use super::SUBCLASS_OF_CLAIM;
use super::Snak;
use super::claim_and_roles;
use super::claim_still_valid;

use std::collections::HashSet;
use std::error::Error;

pub(crate) fn create_tables(conn: &mut rusqlite::Connection) -> Result<(), Box<dyn Error>> {
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
    conn.execute(
        "CREATE TABLE subclass (
            id TEXT not null,
            parent TEXT not null,
            UNIQUE(id, parent)
        );
        ",
        (),
    )?;

    Ok(())
}
pub(crate) struct Statements<'conn> {
    pub(crate) insert_entity: rusqlite::Statement<'conn>,
    insert_position: rusqlite::Statement<'conn>,
    insert_nature: rusqlite::Statement<'conn>,
    insert_edge: rusqlite::Statement<'conn>,
    insert_subclass: rusqlite::Statement<'conn>,
    pub(crate) select_entity: rusqlite::Statement<'conn>,
    pub(crate) select_entities_category: rusqlite::Statement<'conn>,
    pub(crate) select_edges_category: rusqlite::Statement<'conn>,
    pub(crate) top_200_categories_by_edges: rusqlite::Statement<'conn>,
}
impl<'conn> Statements<'conn> {
    pub(crate) fn new(conn: &'conn rusqlite::Connection) -> Self {
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
            insert_subclass: conn
                .prepare(
                    "INSERT OR IGNORE INTO subclass (id, parent)
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
                .prepare("SELECT a.lon, a.lat, b.lon, b.lat FROM edges as edj, natures as nat, positions as a, positions as b WHERE nat.nat = ?1 and edj.a = nat.id and and nat.nat IN (SELECT nat from natures where id = edj.b) and edj.a = a.id and edj.b = b.id;")
                .expect("Failed to prepare select category"),
            top_200_categories_by_edges: conn
                .prepare("SELECT nat.nat, COUNT(edj.rowid) as c FROM edges as edj, natures as nat WHERE edj.a = nat.id AND nat.nat IN (SELECT nat from natures where id = edj.b) GROUP BY nat.nat ORDER BY c DESC LIMIT 200;")
                .expect("Failed to prepare top 200 categories"),
        }
    }
}
pub(crate) fn insert_base<'a>(st: &mut Statements, item: &Element<'a>) {
    let label_en = label(&item.labels, "en").unwrap_or_else(|| label_or_empty(&item.labels, "mul"));
    let label_fr = label_or_empty(&item.labels, "fr");

    st.insert_entity
        .execute((item.id, label_en, label_fr))
        .expect("Failed base insert");
}
pub(crate) fn insert<'a>(st: &mut Statements, item: &Element<'a>) {
    let natures = item
        .claims
        .get(NATURE_CLAIM)
        .unwrap_or_else(|| {
            panic!("No nature for {}", item.id);
        })
        .iter()
        .filter(|nat| claim_still_valid(nat))
        .flat_map(|nat| claim_and_roles(nat))
        .collect::<HashSet<_>>()
        .into_iter();
    // We should not reach this code without an existing position
    let position = item
        .claims
        .get(POSITION_CLAIM)
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
        .get(SHARES_BORDER_WITH_CLAIM)
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

pub(crate) fn insert_subclass<'a>(st: &mut Statements, item: &Element<'a>) {
    item.claims
        .get(SUBCLASS_OF_CLAIM)
        .unwrap_or_else(|| {
            panic!("No subclass for {}", item.id);
        })
        .iter()
        .filter(|claim| claim_still_valid(claim))
        .filter_map(|pos| {
            if let Snak::Item { ref value } = pos.mainsnak {
                Some((item.id, value.id))
            } else {
                None // Ignore elements without subclass id
            }
        })
        .for_each(|(id, parent_id)| {
            st.insert_subclass
                .execute((id, parent_id))
                .expect("Failed subclass insert");
        });
}
