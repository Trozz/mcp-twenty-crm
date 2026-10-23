//! Generic record tools: they work on every object, standard or custom.

use super::{
    Caller, CrmServer, DEFAULT_LIMIT, MAX_BATCH, MAX_LIMIT, data_arg, depth_arg, fields_arg,
    filter_arg, id_arg, json_result, limit_arg, object_arg, order_by_arg, required_text, run,
};
use crate::error::{self, TwentyError};
use crate::view::{page, record, records, unwrap_data};
use axum::http::request::Parts;
use reqwest::Method;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

const SEARCH_DEFAULT_OBJECTS: &[&str] = &["people", "companies", "opportunities"];
const SEARCH_DEFAULT_LIMIT: u32 = 5;
const SEARCH_MAX_LIMIT: u32 = 50;
const SEARCH_MAX_OBJECTS: usize = 8;
const SEARCH_MAX_WORDS: usize = 5;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListRecordsParams {
    /// Plural API name of the object, e.g. people, companies, opportunities, notes, tasks.
    pub object: String,
    /// Twenty filter, e.g. name[ilike]:%acme%, companyId[eq]:<id>, or and(stage[eq]:NEW,createdAt[gte]:2026-01-01). Comparators: eq, neq, in, containsAny, is (NULL or NOT_NULL), gt, gte, lt, lte, startsWith, like, ilike; combine with and(), or(), not(). Composite subfields use dots: emails.primaryEmail, name.lastName, amount.amountMicros.
    pub filter: Option<String>,
    /// Sort, e.g. createdAt[DescNullsLast] or name[AscNullsFirst]. Several are separated by commas.
    pub order_by: Option<String>,
    /// Records per page, 1 to 200. Default: 20.
    pub limit: Option<u32>,
    /// next_cursor from the previous page, to get the next one.
    pub cursor: Option<String>,
    /// Only these fields (id is always included), e.g. ["name", "emails", "companyId"].
    pub fields: Option<Vec<String>>,
    /// 1 to include related records (such as a person's company) in full; 0 or unset gives their ids.
    pub depth: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetRecordParams {
    /// Plural API name of the object, e.g. people.
    pub object: String,
    /// The record's id, from a list or search tool.
    pub id: String,
    /// 1 to include related records in full (such as an opportunity's company and point of contact). Default: 1.
    pub depth: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RecordRefParams {
    /// Plural API name of the object, e.g. people.
    pub object: String,
    /// The record's id, from a list or search tool.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct DeleteRecordParams {
    /// Plural API name of the object, e.g. people.
    pub object: String,
    /// The record's id, from a list or search tool.
    pub id: String,
    /// true destroys the record for good. Default false: it goes to the trash and restore_record can bring it back.
    pub permanent: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateRecordParams {
    /// Plural API name of the object, e.g. companies.
    pub object: String,
    /// The record's fields by API name, as describe_object lists them, e.g. {"name": "Acme", "domainName": {"primaryLinkUrl": "https://acme.com"}}. Relations take the related record's id, e.g. {"companyId": "<id>"}.
    pub data: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateRecordsParams {
    /// Plural API name of the object, e.g. people.
    pub object: String,
    /// 1 to 60 records, each an object of fields as for create_record.
    pub records: Vec<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateRecordParams {
    /// Plural API name of the object, e.g. people.
    pub object: String,
    /// The record's id, from a list or search tool.
    pub id: String,
    /// Only the fields to change, e.g. {"jobTitle": "CTO"} or {"stage": "WON"}. A composite field is replaced as a whole, so give all of its parts you want to keep.
    pub data: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchRecordsParams {
    /// Words to find, e.g. "ann lee" or "acme". Every word must appear in a name (or, for people, an email).
    pub query: String,
    /// Objects to search, by plural API name. Default: people, companies, opportunities. Notes and tasks are searched by title, custom objects by name.
    pub objects: Option<Vec<String>>,
    /// Records per object, 1 to 50. Default: 5.
    pub limit: Option<u32>,
}

/// The fields `search_records` matches in each object.
pub(crate) fn search_fields(object: &str) -> &'static [&'static str] {
    match object {
        "people" => &["name.firstName", "name.lastName", "emails.primaryEmail"],
        "companies" => &["name", "domainName.primaryLinkUrl"],
        "notes" | "tasks" => &["title"],
        "workspaceMembers" => &["name.firstName", "name.lastName", "userEmail"],
        _ => &["name"],
    }
}

/// The words of a search, without the characters of Twenty's filter grammar, so a term cannot
/// leave its clause.
pub(crate) fn search_words(query: &str) -> error::Result<Vec<String>> {
    let cleaned: String = query
        .chars()
        .map(|c| {
            if matches!(
                c,
                '%' | '(' | ')' | ',' | '[' | ']' | '\\' | '"' | '\'' | '_'
            ) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<String> = cleaned.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Err(TwentyError::invalid(
            "query must contain letters or digits to search for",
        ));
    }
    if words.len() > SEARCH_MAX_WORDS {
        return Err(TwentyError::invalid(format!(
            "query is limited to {SEARCH_MAX_WORDS} words"
        )));
    }
    Ok(words)
}

/// A filter matching records where every word is in at least one of `fields`.
pub(crate) fn search_filter(words: &[String], fields: &[&str]) -> String {
    let clause = |word: &str| {
        let parts: Vec<String> = fields
            .iter()
            .map(|f| format!("{f}[ilike]:%{word}%"))
            .collect();
        if parts.len() == 1 {
            parts.into_iter().next().expect("one part")
        } else {
            format!("or({})", parts.join(","))
        }
    };
    let clauses: Vec<String> = words.iter().map(|w| clause(w)).collect();
    if clauses.len() == 1 {
        return clauses.into_iter().next().expect("one clause");
    }
    format!("and({})", clauses.join(","))
}

impl Caller {
    /// One page of `object`, shaped.
    pub(crate) async fn list_page(
        &self,
        object: &str,
        query: &[(&str, String)],
    ) -> error::Result<Value> {
        let found = self.get("list_records", &[object], query).await?;
        Ok(page(object, &found))
    }
}

#[tool_router(router = records_router, vis = "pub(crate)")]
impl CrmServer {
    #[tool(
        description = "List records of any object, one page at a time, with an optional filter and sort. Returns total, count, items and next_cursor while there are more pages. Use describe_object for field names.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_records(
        &self,
        Parameters(p): Parameters<ListRecordsParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_records");
        run("list_records", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let mut q = vec![(
                "limit",
                limit_arg(p.limit, DEFAULT_LIMIT, MAX_LIMIT)?.to_string(),
            )];
            if let Some(f) = filter_arg(p.filter.as_deref())? {
                q.push(("filter", f));
            }
            if let Some(o) = order_by_arg(p.order_by.as_deref())? {
                q.push(("order_by", o));
            }
            if let Some(cursor) = p.cursor.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                q.push(("starting_after", cursor.to_string()));
            }
            if let Some(f) = fields_arg(p.fields.as_deref())? {
                q.push(("fields", f));
            }
            if let Some(d) = depth_arg(p.depth)? {
                q.push(("depth", d.to_string()));
            }
            Ok(json_result(&c.list_page(&object, &q).await?))
        })
        .await
    }

    #[tool(
        description = "Read one record of any object by id, with its related records (depth 1) unless depth is 0.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_record(
        &self,
        Parameters(p): Parameters<GetRecordParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_record");
        run("get_record", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let id = id_arg("id", &p.id)?;
            let depth = depth_arg(p.depth)?.unwrap_or(1);
            let found = c
                .get("get_record", &[&object, &id], &[("depth", depth.to_string())])
                .await
                .map_err(|e| match e {
                    TwentyError::NotFound(_) => TwentyError::NotFound(format!(
                        "no {object} record with id {id}; find it with search_records or list_records"
                    )),
                    other => other,
                })?;
            Ok(json_result(&record(unwrap_data(&found))))
        })
        .await
    }

    #[tool(
        description = "Find records by name across objects (default people, companies and opportunities; people also by email). Every word of the query must match. Returns up to limit records per object; use list_records with a filter for more.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn search_records(
        &self,
        Parameters(p): Parameters<SearchRecordsParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_records");
        run("search_records", async {
            let c = self.caller(&parts)?;
            let words = search_words(&required_text("query", &p.query)?)?;
            let limit = limit_arg(p.limit, SEARCH_DEFAULT_LIMIT, SEARCH_MAX_LIMIT)?;
            let objects: Vec<String> = match p.objects.as_deref() {
                Some(list) if !list.is_empty() => {
                    let mut out: Vec<String> = Vec::new();
                    for o in list {
                        let o = object_arg(o)?;
                        if !out.contains(&o) {
                            out.push(o);
                        }
                    }
                    out
                }
                _ => SEARCH_DEFAULT_OBJECTS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            };
            if objects.len() > SEARCH_MAX_OBJECTS {
                return Err(TwentyError::invalid(format!(
                    "objects is limited to {SEARCH_MAX_OBJECTS} names"
                )));
            }
            let mut out = Map::new();
            for object in &objects {
                let q = vec![
                    ("limit", limit.to_string()),
                    ("filter", search_filter(&words, search_fields(object))),
                ];
                let entry = match c.get("search_records", &[object], &q).await {
                    Ok(found) => {
                        let items = records(&found);
                        let mut v = json!({"count": items.len(), "items": items});
                        if let Some(total) = found.get("totalCount").and_then(Value::as_u64) {
                            v["total"] = Value::from(total);
                        }
                        v
                    }
                    // One object failing (say, a role that cannot read it) leaves the others.
                    Err(e @ TwentyError::RateLimited { .. }) | Err(e @ TwentyError::Auth) => {
                        return Err(e);
                    }
                    Err(e) => json!({"error": format!("{}: {e}", e.kind())}),
                };
                out.insert(object.clone(), entry);
            }
            Ok(json_result(&Value::Object(out)))
        })
        .await
    }

    #[tool(
        description = "Create one record of any object from its own fields (see describe_object). Returns the new record with its id. For people, companies, opportunities, notes and tasks the create_ tools are simpler.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_record(
        &self,
        Parameters(p): Parameters<CreateRecordParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_record", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let data = data_arg("data", &p.data)?;
            Ok(json_result(&c.create(&object, data).await?))
        })
        .await
    }

    #[tool(
        description = "Create 1 to 60 records of one object in a single request, e.g. to import a list. Returns the new records with their ids.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_records(
        &self,
        Parameters(p): Parameters<CreateRecordsParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_records", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            if p.records.is_empty() || p.records.len() > MAX_BATCH {
                return Err(TwentyError::invalid(format!(
                    "records must hold 1 to {MAX_BATCH} records; split larger imports"
                )));
            }
            let body = Value::Array(
                p.records
                    .iter()
                    .map(|r| data_arg("each record", r))
                    .collect::<error::Result<Vec<_>>>()?,
            );
            let found = c
                .send(
                    Method::POST,
                    "create_records",
                    &["batch", &object],
                    &[],
                    Some(&body),
                )
                .await?;
            let created = records(&found);
            Ok(json_result(
                &json!({"object": object, "count": created.len(), "items": created}),
            ))
        })
        .await
    }

    #[tool(
        description = "Change fields of one record of any object. Only the fields given change. Returns the updated record.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn update_record(
        &self,
        Parameters(p): Parameters<UpdateRecordParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("update_record", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let id = id_arg("id", &p.id)?;
            let data = data_arg("data", &p.data)?;
            let found = c
                .send(
                    Method::PATCH,
                    "update_record",
                    &[&object, &id],
                    &[],
                    Some(&data),
                )
                .await?;
            Ok(json_result(&record(unwrap_data(&found))))
        })
        .await
    }

    #[tool(
        description = "Delete one record. By default it goes to the trash, where restore_record can bring it back; permanent true destroys it for good. Confirm with the person before deleting.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn delete_record(
        &self,
        Parameters(p): Parameters<DeleteRecordParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("delete_record", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let id = id_arg("id", &p.id)?;
            let permanent = p.permanent.unwrap_or(false);
            // Twenty destroys a record unless soft_delete is exactly "true".
            let soft = if permanent { "false" } else { "true" };
            c.send(
                Method::DELETE,
                "delete_record",
                &[&object, &id],
                &[("soft_delete", soft.to_string())],
                None,
            )
            .await?;
            Ok(json_result(&json!({
                "object": object,
                "id": id,
                "deleted": if permanent { "permanently" } else { "to the trash; restore_record brings it back" },
            })))
        })
        .await
    }

    #[tool(
        description = "Bring a record back from the trash after delete_record without permanent.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub async fn restore_record(
        &self,
        Parameters(p): Parameters<RecordRefParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("restore_record", async {
            let c = self.caller(&parts)?;
            let object = object_arg(&p.object)?;
            let id = id_arg("id", &p.id)?;
            let found = c
                .send(
                    Method::PATCH,
                    "restore_record",
                    &[&object, &id, "restore"],
                    &[],
                    None,
                )
                .await?;
            Ok(json_result(&record(unwrap_data(&found))))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_terms_cannot_leave_their_clause() {
        let words = search_words(r#"acme),or(id[neq]"x%"#).unwrap();
        assert_eq!(words, vec!["acme", "or", "id", "neq", "x"]);
        let filter = search_filter(&words[..1], search_fields("companies"));
        assert_eq!(
            filter,
            "or(name[ilike]:%acme%,domainName.primaryLinkUrl[ilike]:%acme%)"
        );
        for w in &words {
            assert!(!w.contains(['(', ')', ',', '[', ']', '%', '"']), "{w}");
        }
    }

    #[test]
    fn every_word_must_match_some_field() {
        let words = search_words("Ann  Lee").unwrap();
        assert_eq!(
            search_filter(&words, &["name"]),
            "and(name[ilike]:%Ann%,name[ilike]:%Lee%)"
        );
        assert_eq!(
            search_filter(&words[..1], search_fields("people")),
            "or(name.firstName[ilike]:%Ann%,name.lastName[ilike]:%Ann%,emails.primaryEmail[ilike]:%Ann%)"
        );
    }

    #[test]
    fn empty_or_long_queries_are_refused() {
        assert!(search_words(" %% () ").is_err());
        assert!(search_words("a b c d e f").is_err());
    }
}
