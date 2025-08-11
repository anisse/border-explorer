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

pub(crate) fn create_tables(
    conn: &mut rusqlite::Connection,
    banned_categories: &HashSet<u64>,
) -> Result<(), Box<dyn Error>> {
    conn.execute(
        "CREATE TABLE entities (
            id INTEGER PRIMARY KEY,
            name_en TEXT,
            name_fr TEXT
        );",
        (),
    )?;
    conn.execute(
        "CREATE TABLE positions (
            id INTEGER PRIMARY KEY,
            lat TEXT,
            lon TEXT,
            FOREIGN KEY(id) REFERENCES entities(id)
        );",
        (),
    )?;
    conn.execute(
        "CREATE TABLE natures (
            id INTEGER,
            nat INTEGER,
            FOREIGN KEY(id) REFERENCES entities(id)
        );
        ",
        (),
    )?;
    conn.execute("CREATE INDEX natures_nat ON natures(nat);", ())?;
    conn.execute("CREATE INDEX natures_id_nat ON natures(id, nat);", ())?;
    conn.execute(
        "CREATE TABLE edges (
            a INTEGER NOT NULL,
            b INTEGER NOT NULL,
            UNIQUE(a, b)
        );",
        (),
    )?;
    conn.execute("CREATE INDEX edges_a ON edges(a);", ())?;
    conn.execute("CREATE INDEX edges_b ON edges(b);", ())?;
    conn.execute(
        "CREATE TABLE subclass (
            id INTEGER NOT NULL,
            parent INTEGER NOT NULL,
            UNIQUE(id, parent)
            FOREIGN KEY(id) REFERENCES entities(id)
        );
        ",
        (),
    )?;
    conn.execute("CREATE TABLE banned_natures (id INTEGER NOT NULL);", ())?;
    conn.execute("CREATE INDEX subclass_parent ON subclass(parent);", ())?;
    conn.execute(
        ("INSERT INTO banned_natures VALUES ".to_string()
            + &banned_categories
                .iter()
                .map(|c| format!("({c})"))
                .collect::<Vec<String>>()
                .join(",")
            + ";")
            .as_str(),
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
    pub(crate) top_categories_by_edges: rusqlite::Statement<'conn>,
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
                .prepare("
                WITH all_children(nat) AS (
                    VALUES(?1)
                    UNION SELECT sub.id FROM subclass AS sub JOIN all_children ON all_children.nat = sub.parent)
                SELECT DISTINCT e.name_en, e.name_fr, p.lon, p.lat
                    FROM entities AS e, positions AS p, natures as nat
                    WHERE p.id = e.id AND nat.id = e.id AND nat.nat IN all_children
                    ORDER BY e.id;")
                .expect("Failed to prepare select category"),
            select_edges_category: conn
                .prepare("
                WITH all_children(nat) AS (
                    VALUES(?1)
                    UNION SELECT sub.id FROM subclass AS sub JOIN all_children ON all_children.nat = sub.parent)
                SELECT DISTINCT a.lon, a.lat, b.lon, b.lat
                    FROM edges AS edj, positions AS a, positions AS b, natures as anat, natures as bnat
                    WHERE edj.a = anat.id AND anat.nat in all_children AND bnat.id = edj.b AND bnat.nat IN all_children
                        AND edj.a = a.id AND edj.b = b.id
                    ORDER BY edj.a;")
                .expect("Failed to prepare select category"),
            top_categories_by_edges: conn
                .prepare("
                WITH all_parents(id, nat) AS (
                    SELECT DISTINCT natures.id, natures.nat FROM natures WHERE nat NOT IN banned_natures
                    UNION SELECT all_parents.id, sub.parent FROM subclass AS sub
                        JOIN all_parents ON all_parents.nat = sub.id WHERE sub.parent NOT IN banned_natures)
                SELECT ap.nat, COUNT(edj.rowid) AS c FROM edges as edj, all_parents as ap
                    WHERE edj.a = ap.id AND ap.nat IN (SELECT nat FROM all_parents WHERE id = edj.b)
                    GROUP BY ap.nat HAVING c >= 28 AND 10 * c / COUNT(distinct ap.id) >= 18 ORDER BY c DESC LIMIT 600;")
                .expect("Failed to prepare top categories"),
        }
    }
}
pub(crate) fn insert_base<'a>(st: &mut Statements, item: &Element<'a>) {
    let label_en = label(&item.labels, "en").unwrap_or_else(|| label_or_empty(&item.labels, "mul"));
    let label_fr = label_or_empty(&item.labels, "fr");

    st.insert_entity
        .execute((int_id(item.id), label_en, label_fr))
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
                Some(int_id(value.id))
            } else {
                None // Ignore elements explicitly without any item to share border with, like Q71356
            }
        });
    st.insert_position
        .execute((int_id(item.id), position.latitude, position.longitude))
        .expect("Failed insert");
    natures.for_each(|nat| {
        st.insert_nature
            .execute((int_id(item.id), nat))
            .expect("Failed nature insert");
    });
    connections.for_each(|edge| {
        let mut items = [int_id(item.id), edge];
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
                Some((int_id(item.id), int_id(value.id)))
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

pub(crate) fn int_id(id: &str) -> u64 {
    if id.bytes().next() != Some(b'Q') {
        panic!("Not a Q-entity: {id}");
    }
    let integer_part = &id[1..];
    integer_part
        .parse()
        .unwrap_or_else(|e| panic!("Not an int '{integer_part}': {e}"))
}
