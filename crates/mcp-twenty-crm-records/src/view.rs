//! Shapes Twenty responses into the compact JSON the tools return: envelopes unwrapped, empty
//! values dropped, and pages labelled with the cursor for the next one.

use serde_json::{Map, Value, json};

/// Record members that are noise to a reader, or carry a signed file token (`avatarUrl`).
const RECORD_DROPPED: &[&str] = &["searchVector", "avatarUrl"];

/// Removes nulls, empty strings and empty arrays or objects, recursively. `None` when nothing is
/// left.
pub fn prune(v: Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::Array(items) => {
            let items: Vec<Value> = items.into_iter().filter_map(prune).collect();
            (!items.is_empty()).then_some(Value::Array(items))
        }
        Value::Object(map) => {
            let map: Map<String, Value> = map
                .into_iter()
                .filter_map(|(k, v)| prune(v).map(|v| (k, v)))
                .collect();
            (!map.is_empty()).then_some(Value::Object(map))
        }
        other => Some(other),
    }
}

/// The payload of a Twenty REST answer: `{"data": {"createPerson": {...}}}` is the person,
/// `{"data": {"people": [...]}}` the list. Anything else is returned as it is.
pub fn unwrap_data(v: &Value) -> &Value {
    let Some(data) = v.get("data") else {
        return v;
    };
    match data.as_object() {
        Some(map) if map.len() == 1 => map.values().next().expect("one value"),
        _ => data,
    }
}

/// One record, without empty values or search internals, also in its related records.
pub fn record(v: &Value) -> Value {
    let mut v = v.clone();
    drop_internals(&mut v);
    prune(v).unwrap_or_else(|| json!({}))
}

fn drop_internals(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for k in RECORD_DROPPED {
                map.remove(*k);
            }
            map.values_mut().for_each(drop_internals);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_internals),
        _ => {}
    }
}

/// The records of a list answer.
pub fn records(found: &Value) -> Vec<Value> {
    unwrap_data(found)
        .as_array()
        .map(|items| items.iter().map(record).collect())
        .unwrap_or_default()
}

/// A page of records: `items`, `count`, `total` when Twenty gave it, and `next_cursor` while
/// there are more.
pub fn page(object: &str, found: &Value) -> Value {
    let items = records(found);
    let mut out = Map::new();
    out.insert("object".into(), Value::from(object));
    if let Some(total) = found.get("totalCount").and_then(Value::as_u64) {
        out.insert("total".into(), Value::from(total));
    }
    out.insert("count".into(), Value::from(items.len()));
    let more = found
        .pointer("/pageInfo/hasNextPage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cursor = found
        .pointer("/pageInfo/endCursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty());
    if let (true, Some(cursor)) = (more, cursor) {
        out.insert("next_cursor".into(), Value::from(cursor));
    }
    out.insert("items".into(), Value::Array(items));
    Value::Object(out)
}

/// The array of a metadata list answer, which Twenty has sent as `{"data": {"objects": [...]}}`,
/// `{"objects": [...]}`, a bare array, or GraphQL-style `{"edges": [{"node": ...}]}`.
pub fn list_items(v: &Value) -> Vec<&Value> {
    let inner = unwrap_data(v);
    let inner = inner
        .get("objects")
        .or_else(|| inner.get("fields"))
        .unwrap_or(inner);
    if let Some(items) = inner.as_array() {
        return items.iter().collect();
    }
    if let Some(edges) = inner.get("edges").and_then(Value::as_array) {
        return edges.iter().filter_map(|e| e.get("node")).collect();
    }
    Vec::new()
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Whether an object or field is in use: active unless Twenty says otherwise.
pub fn is_active(v: &Value) -> bool {
    v.get("isActive").and_then(Value::as_bool).unwrap_or(true)
}

pub fn is_system(v: &Value) -> bool {
    flag(v, "isSystem")
}

/// One object of the data model as the tools present it.
pub fn object_summary(o: &Value) -> Value {
    let mut v = json!({
        "name": o.get("namePlural"),
        "singular": o.get("nameSingular"),
        "label": o.get("labelPlural").or_else(|| o.get("labelSingular")),
        "description": o.get("description"),
    });
    if flag(o, "isCustom") {
        v["custom"] = Value::Bool(true);
    }
    if is_system(o) {
        v["system"] = Value::Bool(true);
    }
    prune(v).unwrap_or_else(|| json!({}))
}

/// One field of an object: name, type, label, whether it may be empty, select options and, for
/// relations, where they point.
pub fn field_summary(f: &Value) -> Value {
    let options: Option<Vec<Value>> = f.get("options").and_then(Value::as_array).map(|opts| {
        opts.iter()
            .filter_map(|o| o.get("value").cloned())
            .collect()
    });
    let target = f
        .pointer("/relation/targetObjectMetadata/nameSingular")
        .or_else(|| f.pointer("/relationDefinition/targetObjectMetadata/nameSingular"))
        .cloned();
    let relation_type = f
        .pointer("/settings/relationType")
        .or_else(|| f.pointer("/relation/type"))
        .cloned();
    let mut v = json!({
        "name": f.get("name"),
        "type": f.get("type"),
        "label": f.get("label"),
        "description": f.get("description"),
        "options": options,
        "relation_to": target,
        "relation_type": relation_type,
        "set_with": f.pointer("/settings/joinColumnName"),
    });
    if f.get("isNullable").and_then(Value::as_bool) == Some(false) {
        v["required"] = Value::Bool(true);
    }
    if flag(f, "isCustom") {
        v["custom"] = Value::Bool(true);
    }
    prune(v).unwrap_or_else(|| json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_are_unwrapped() {
        let created = json!({"data": {"createPerson": {"id": "1"}}});
        assert_eq!(unwrap_data(&created), &json!({"id": "1"}));
        let bare = json!({"id": "2"});
        assert_eq!(unwrap_data(&bare), &bare);
        let two = json!({"data": {"a": 1, "b": 2}});
        assert_eq!(unwrap_data(&two), &json!({"a": 1, "b": 2}));
    }

    #[test]
    fn records_drop_empty_values_and_search_internals() {
        let r = json!({
            "id": "1",
            "name": {"firstName": "Ann", "lastName": ""},
            "emails": {"primaryEmail": "", "additionalEmails": []},
            "city": null,
            "searchVector": "'ann':1",
            "position": 0,
            "isActive": false
        });
        assert_eq!(
            record(&r),
            json!({"id": "1", "name": {"firstName": "Ann"}, "position": 0, "isActive": false})
        );
    }

    #[test]
    fn pages_carry_the_next_cursor_only_while_there_are_more() {
        let found = json!({
            "data": {"people": [{"id": "1"}, {"id": "2"}]},
            "totalCount": 5,
            "pageInfo": {"hasNextPage": true, "endCursor": "c2", "startCursor": "c1"}
        });
        assert_eq!(
            page("people", &found),
            json!({"object": "people", "total": 5, "count": 2, "next_cursor": "c2", "items": [{"id": "1"}, {"id": "2"}]})
        );
        let last = json!({
            "data": {"people": [{"id": "5"}]},
            "totalCount": 5,
            "pageInfo": {"hasNextPage": false, "endCursor": "c5"}
        });
        assert!(page("people", &last).get("next_cursor").is_none());
    }

    #[test]
    fn metadata_lists_are_read_in_every_shape() {
        let a = json!({"data": {"objects": [{"id": 1}]}});
        let b = json!({"objects": [{"id": 1}]});
        let c = json!([{"id": 1}]);
        let d = json!({"data": {"objects": {"edges": [{"node": {"id": 1}}]}}});
        for v in [a, b, c, d] {
            assert_eq!(list_items(&v), vec![&json!({"id": 1})], "{v}");
        }
    }

    #[test]
    fn fields_are_summarised() {
        let f = json!({
            "name": "stage",
            "type": "SELECT",
            "label": "Stage",
            "isNullable": false,
            "options": [{"value": "NEW", "label": "New"}, {"value": "WON", "label": "Won"}]
        });
        assert_eq!(
            field_summary(&f),
            json!({"name": "stage", "type": "SELECT", "label": "Stage", "options": ["NEW", "WON"], "required": true})
        );
        let r = json!({
            "name": "company",
            "type": "RELATION",
            "isNullable": true,
            "settings": {"relationType": "MANY_TO_ONE", "joinColumnName": "companyId"},
            "relation": {"targetObjectMetadata": {"nameSingular": "company"}}
        });
        assert_eq!(
            field_summary(&r),
            json!({"name": "company", "type": "RELATION", "relation_to": "company", "relation_type": "MANY_TO_ONE", "set_with": "companyId"})
        );
    }
}
