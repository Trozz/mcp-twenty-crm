//! The workspace's data model: which objects exist and what fields they have.

use super::{Caller, CrmServer, json_result, object_arg, run};
use crate::error::{self, TwentyError};
use crate::view::{field_summary, is_active, is_system, list_items, object_summary, unwrap_data};
use axum::http::request::Parts;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListObjectsParams {
    /// Also list Twenty's internal system objects. Default: false.
    pub include_system: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct DescribeObjectParams {
    /// Plural or singular API name of the object, e.g. people or person.
    pub object: String,
    /// Also list system fields such as createdBy and position. Default: false.
    pub include_system: Option<bool>,
}

fn name_of<'a>(o: &'a Value, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

impl Caller {
    /// Every object of the workspace's data model, as Twenty describes it.
    async fn objects(&self) -> error::Result<Vec<Value>> {
        let found = self
            .get("metadata_objects", &["metadata", "objects"], &[])
            .await?;
        Ok(list_items(&found).into_iter().cloned().collect())
    }
}

#[tool_router(router = metadata_router, vis = "pub(crate)")]
impl CrmServer {
    #[tool(
        description = "List the objects of this workspace's data model (people, companies, opportunities, notes, tasks and custom objects): plural API name to use as object in the other tools, singular name, label and description. Needs an API key whose role may read the data model.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_objects(
        &self,
        Parameters(p): Parameters<ListObjectsParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("list_objects", async {
            let c = self.caller(&parts)?;
            let system = p.include_system.unwrap_or(false);
            let mut items: Vec<Value> = c
                .objects()
                .await?
                .iter()
                .filter(|o| is_active(o) && (system || !is_system(o)))
                .map(object_summary)
                .collect();
            items.sort_by(|a, b| {
                a["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["name"].as_str().unwrap_or_default())
            });
            Ok(json_result(&json!({"count": items.len(), "items": items})))
        })
        .await
    }

    #[tool(
        description = "Describe one object's fields: API name, type (TEXT, NUMBER, SELECT, CURRENCY, FULL_NAME, EMAILS, PHONES, LINKS, ADDRESS, RELATION and others), label, whether it is required, the values a SELECT or MULTI_SELECT accepts, and for a RELATION its kind and set_with, the field to set with the related record's id (e.g. companyId). Use it before filtering, creating or updating.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn describe_object(
        &self,
        Parameters(p): Parameters<DescribeObjectParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("describe_object", async {
            let c = self.caller(&parts)?;
            let wanted = object_arg(&p.object)?;
            let objects = c.objects().await?;
            let object = objects
                .iter()
                .find(|o| {
                    name_of(o, "namePlural") == Some(wanted.as_str())
                        || name_of(o, "nameSingular") == Some(wanted.as_str())
                })
                .ok_or_else(|| {
                    TwentyError::NotFound(format!(
                        "no object named {wanted}; list_objects lists the objects of this workspace"
                    ))
                })?;
            let mut fields: Vec<&Value> = list_items(object.get("fields").unwrap_or(&Value::Null));
            let detail;
            if fields.is_empty()
                && let Some(id) = name_of(object, "id")
            {
                detail = c
                    .get("metadata_object", &["metadata", "objects", id], &[])
                    .await?;
                fields = list_items(unwrap_data(&detail).get("fields").unwrap_or(&Value::Null));
            }
            let system = p.include_system.unwrap_or(false);
            let mut out_fields: Vec<Value> = fields
                .into_iter()
                .filter(|f| is_active(f) && (system || !is_system(f)))
                .map(field_summary)
                .collect();
            out_fields.sort_by(|a, b| {
                a["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["name"].as_str().unwrap_or_default())
            });
            let mut out = object_summary(object);
            out["fields"] = Value::Array(out_fields);
            Ok(json_result(&out))
        })
        .await
    }
}
