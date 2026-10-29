//! Reading the JSON documents of a project file without losing what cannot
//! be read. A value this version does not understand (a new kind of status
//! from a newer version, a hand-edited typo) would otherwise fail the whole
//! document: the project would open without its issues or decisions and the
//! next save would write them away. Instead each field, and each entry of
//! the list fields, is read on its own; what fails is left out with a
//! message, kept as it was, and written back unchanged on save.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// What could not be read, kept to be written back.
#[derive(Debug, Clone, Default)]
pub struct Leftovers {
    /// Whole fields, as they were.
    fields: Vec<(String, Value)>,
    /// Entries of list fields, as they were.
    entries: Vec<(String, Value)>,
}

impl Leftovers {
    /// Puts the unread parts back into a document about to be saved. A
    /// field is only put back while it still has its default value, so a
    /// field set again since is not overwritten.
    pub fn restore<T: Default + Serialize>(&self, json: &mut Value) {
        let Some(object) = json.as_object_mut() else {
            return;
        };
        let defaults = serde_json::to_value(T::default()).unwrap_or(Value::Null);
        for (key, value) in &self.fields {
            let current = object.get(key);
            if current.is_none() || current == defaults.get(key) {
                object.insert(key.clone(), value.clone());
            }
        }
        for (key, value) in &self.entries {
            match object.get_mut(key) {
                Some(Value::Array(list)) => list.push(value.clone()),
                _ => {
                    object.insert(key.clone(), Value::Array(vec![value.clone()]));
                }
            }
        }
    }
}

/// Reads `json` as `T`, field by field, and the fields named in `lists`
/// entry by entry. `T` must have `#[serde(default)]`. Returns what was read,
/// what was not, and a message per part left out, starting with `what`.
pub fn read<T: DeserializeOwned + Default>(
    json: Value,
    lists: &[&str],
    what: &str,
) -> (T, Leftovers, Vec<String>) {
    let mut leftovers = Leftovers::default();
    let mut errors = Vec::new();
    let Value::Object(object) = json else {
        errors.push(format!("{what} could not be read: not a JSON object"));
        return (T::default(), leftovers, errors);
    };
    let single = |key: &str, value: Value| {
        let mut one = Map::new();
        one.insert(key.to_owned(), value);
        serde_json::from_value::<T>(Value::Object(one))
    };
    let mut kept = Map::new();
    for (key, value) in object {
        if lists.contains(&key.as_str())
            && let Value::Array(list) = value
        {
            let mut good = Vec::new();
            for entry in list {
                match single(&key, Value::Array(vec![entry.clone()])) {
                    Ok(_) => good.push(entry),
                    Err(e) => {
                        errors.push(format!(
                            "{what}: an entry in \"{key}\" could not be read and was left out \
                             (it is kept in the file): {e}"
                        ));
                        leftovers.entries.push((key.clone(), entry));
                    }
                }
            }
            kept.insert(key, Value::Array(good));
            continue;
        }
        match single(&key, value.clone()) {
            Ok(_) => {
                kept.insert(key, value);
            }
            Err(e) => {
                errors.push(format!(
                    "{what}: \"{key}\" could not be read and was left out (it is kept in the file): {e}"
                ));
                leftovers.fields.push((key, value));
            }
        }
    }
    let value = serde_json::from_value(Value::Object(kept)).unwrap_or_else(|e| {
        errors.push(format!("{what} could not be read: {e}"));
        T::default()
    });
    (value, leftovers, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
    #[serde(default)]
    struct Doc {
        items: Vec<Kind>,
        mode: Kind,
        name: String,
    }

    #[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
    enum Kind {
        #[default]
        A,
        B,
    }

    #[test]
    fn unreadable_parts_are_left_out_and_written_back() {
        let json = json!({"items": ["A", "Z", "B"], "mode": "Q", "name": "x"});
        let (doc, leftovers, errors) = read::<Doc>(json, &["items"], "doc.json");
        assert_eq!(
            doc,
            Doc {
                items: vec![Kind::A, Kind::B],
                mode: Kind::A,
                name: "x".into()
            }
        );
        assert_eq!(errors.len(), 2);
        let mut saved = serde_json::to_value(&doc).unwrap();
        leftovers.restore::<Doc>(&mut saved);
        assert_eq!(
            saved,
            json!({"items": ["A", "B", "Z"], "mode": "Q", "name": "x"})
        );
    }

    #[test]
    fn a_field_set_again_is_not_overwritten() {
        let (mut doc, leftovers, _) = read::<Doc>(json!({"mode": "Q"}), &[], "doc.json");
        doc.mode = Kind::B;
        let mut saved = serde_json::to_value(&doc).unwrap();
        leftovers.restore::<Doc>(&mut saved);
        assert_eq!(saved["mode"], json!("B"));
    }
}
