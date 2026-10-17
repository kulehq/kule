//! JSON and TOML, as keys: each key down to [`KEY_DEPTH`] is an entity, and
//! below that a value is compared whole. Values are compared as data, so a
//! file reformatted or with its keys reordered has not changed.

use std::collections::BTreeMap;

use crate::{Entity, Kind, Skip, tidy};

/// Keys this deep are entities; what is below them is their value.
const KEY_DEPTH: usize = 3;

// Both parsers refuse deep nesting (serde_json past 128 levels), which also
// bounds the recursion here; `deep_files_are_refused` keeps that so.

/// A parsed file reduced to what is compared: tables and the rest.
enum Value {
    Table(BTreeMap<String, Value>),
    /// The value written out the same way on both sides.
    Leaf(String),
}

pub(crate) fn json(src: &[u8]) -> Result<Vec<Entity>, Skip> {
    let value: serde_json::Value = serde_json::from_slice(src).map_err(|_| Skip::Unparsed)?;
    Ok(entities(from_json(value)))
}

pub(crate) fn toml(src: &[u8]) -> Result<Vec<Entity>, Skip> {
    let text = std::str::from_utf8(src).map_err(|_| Skip::Unparsed)?;
    let table: toml::Table = text.parse().map_err(|_| Skip::Unparsed)?;
    Ok(entities(from_toml(toml::Value::Table(table))))
}

fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Object(map) => Value::Table(
            map.into_iter()
                .map(|(key, value)| (key, from_json(value)))
                .collect(),
        ),
        other => Value::Leaf(other.to_string()),
    }
}

fn from_toml(value: toml::Value) -> Value {
    match value {
        toml::Value::Table(table) => Value::Table(
            table
                .into_iter()
                .map(|(key, value)| (key, from_toml(value)))
                .collect(),
        ),
        other => Value::Leaf(other.to_string()),
    }
}

fn entities(root: Value) -> Vec<Entity> {
    let mut entities = Vec::new();
    match root {
        Value::Leaf(leaf) => entities.push(Entity::top(leaf.into_bytes())),
        Value::Table(table) => {
            entities.push(Entity::top(Vec::new()));
            add(&mut entities, 0, "", table, 1);
        }
    }
    entities
}

fn add(
    entities: &mut Vec<Entity>,
    parent: usize,
    prefix: &str,
    table: BTreeMap<String, Value>,
    depth: usize,
) {
    for (key, value) in table {
        let name = tidy(&format!("{prefix}{key}"));
        let index = entities.len();
        let (own, inside) = match value {
            Value::Table(table) if depth < KEY_DEPTH => (Vec::new(), Some(table)),
            Value::Table(table) => (whole(&Value::Table(table)).into_bytes(), None),
            Value::Leaf(leaf) => (leaf.into_bytes(), None),
        };
        entities.push(Entity {
            kind: Kind::Key,
            name: name.clone(),
            parent: Some(parent),
            line: None,
            own,
        });
        if let Some(table) = inside {
            add(entities, index, &format!("{name}."), table, depth + 1);
        }
    }
}

/// A table below [`KEY_DEPTH`], written out to be compared whole.
fn whole(value: &Value) -> String {
    match value {
        Value::Leaf(leaf) => leaf.clone(),
        Value::Table(table) => {
            let items: Vec<String> = table
                .iter()
                .map(|(key, value)| format!("{key:?}:{}", whole(value)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::changes;
    use crate::{Edit, Kind, Outline, Skip, diff};

    use Edit::*;

    fn c(edit: Edit, name: &str) -> (Edit, Kind, String) {
        (edit, Kind::Key, name.to_owned())
    }

    #[test]
    fn toml_keys() {
        let before =
            "[package]\nname = \"kule\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n";
        let after = "[package]\nname = \"kule\"\nversion = \"0.2.0\"\n\n[dependencies]\nserde = \"1\"\ntoml = \"1\"\n";
        assert_eq!(
            changes("Cargo.toml", before, after),
            vec![
                c(Added, "dependencies.toml"),
                c(Modified, "package.version")
            ]
        );
    }

    #[test]
    fn json_is_compared_as_data() {
        let before = "{\"a\": 1, \"b\": {\"c\": [1, 2]}}";
        let after = "{\n  \"b\": { \"c\": [1, 2] },\n  \"a\": 1\n}\n";
        assert_eq!(changes("package.json", before, after), vec![]);
        let after = "{\"a\": 1, \"b\": {\"c\": [1, 3]}}";
        assert_eq!(
            changes("package.json", before, after),
            vec![c(Modified, "b.c")]
        );
    }

    #[test]
    fn deep_values_are_compared_whole() {
        let before = "{\"a\": {\"b\": {\"c\": {\"d\": 1}}}}";
        let after = "{\"a\": {\"b\": {\"c\": {\"d\": 2}}}}";
        assert_eq!(changes("x.json", before, after), vec![c(Modified, "a.b.c")]);
    }

    #[test]
    fn a_new_table_comes_with_its_keys() {
        assert_eq!(
            changes("a.toml", "x = 1\n", "x = 1\n[t]\na = 1\nb = 2\n"),
            vec![c(Added, "t")]
        );
    }

    #[test]
    fn deep_files_are_refused() {
        let unparsed = Outline::Skipped {
            reason: Skip::Unparsed,
        };
        let n = 100_000;
        for (path, deep) in [
            ("a.toml", format!("a = {}1{}", "[".repeat(n), "]".repeat(n))),
            (
                "a.toml",
                format!("a = {}1{}", "{b = ".repeat(n), "}".repeat(n)),
            ),
            ("a.json", format!("{}1{}", "[".repeat(n), "]".repeat(n))),
            (
                "a.json",
                format!("{}1{}", "{\"b\":".repeat(n), "}".repeat(n)),
            ),
        ] {
            assert_eq!(diff(path, None, Some(deep.as_bytes())), unparsed, "{path}");
        }
    }

    #[test]
    fn broken_files_are_skipped() {
        assert_eq!(
            diff("a.json", None, Some(b"{,")),
            Outline::Skipped {
                reason: Skip::Unparsed
            }
        );
    }
}
