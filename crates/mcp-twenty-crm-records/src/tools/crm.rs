//! Tools for the standard CRM objects that take plain fields and build Twenty's composite fields,
//! and the notes and tasks linked to a record.

use super::{
    CrmServer, DEFAULT_LIMIT, MAX_LIMIT, id_arg, ids_arg, json_result, limit_arg, required_text,
    run, text_arg,
};
use crate::config::currency_code;
use crate::error::{self, TwentyError};
use crate::view::{record, unwrap_data};
use axum::http::request::Parts;
use reqwest::Method;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Longest note or task body, in characters.
const MAX_BODY_CHARS: usize = 100_000;
/// Largest amount accepted, so that amountMicros stays exact.
const MAX_AMOUNT: f64 = 1e12;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreatePersonParams {
    pub first_name: String,
    pub last_name: Option<String>,
    pub email: Option<String>,
    /// Phone number as dialled within its country, e.g. 7700 900123.
    pub phone: Option<String>,
    /// International calling code for phone, e.g. +44.
    pub phone_calling_code: Option<String>,
    pub job_title: Option<String>,
    pub city: Option<String>,
    /// The person's company, by id from search_records.
    pub company_id: Option<String>,
    pub linkedin_url: Option<String>,
    /// Any other fields by API name, including custom ones (see describe_object people).
    pub extra: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateCompanyParams {
    pub name: String,
    /// Website, e.g. https://acme.com.
    pub domain: Option<String>,
    pub employees: Option<u32>,
    pub linkedin_url: Option<String>,
    pub address_street: Option<String>,
    pub city: Option<String>,
    pub postcode: Option<String>,
    pub country: Option<String>,
    /// The workspace member who owns the account, by id from list_workspace_members.
    pub account_owner_id: Option<String>,
    /// Any other fields by API name, including custom ones (see describe_object companies).
    pub extra: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateOpportunityParams {
    pub name: String,
    /// Deal value in currency units, e.g. 12500.50.
    pub amount: Option<f64>,
    /// ISO 4217 code for amount, e.g. GBP. Default: the server's default currency.
    pub currency_code: Option<String>,
    /// Pipeline stage, one of the stage values describe_object opportunities lists, e.g. NEW, SCREENING, MEETING, PROPOSAL, CUSTOMER.
    pub stage: Option<String>,
    /// Expected close date as an ISO 8601 date-time, e.g. 2026-06-30T00:00:00Z.
    pub close_date: Option<String>,
    /// The company, by id from search_records.
    pub company_id: Option<String>,
    /// The main contact, a person's id from search_records.
    pub point_of_contact_id: Option<String>,
    /// Any other fields by API name, including custom ones (see describe_object opportunities).
    pub extra: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateNoteParams {
    pub title: String,
    /// The note in Markdown.
    pub body: Option<String>,
    /// People to link the note to, by id.
    pub person_ids: Option<Vec<String>>,
    /// Companies to link the note to, by id.
    pub company_ids: Option<Vec<String>>,
    /// Opportunities to link the note to, by id.
    pub opportunity_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateTaskParams {
    pub title: String,
    /// Details in Markdown.
    pub body: Option<String>,
    /// When it is due, as an ISO 8601 date-time, e.g. 2026-03-10T09:00:00Z.
    pub due_at: Option<String>,
    /// TODO, IN_PROGRESS or DONE. Default: TODO.
    pub status: Option<String>,
    /// Who does it: a workspace member's id from list_workspace_members.
    pub assignee_id: Option<String>,
    /// People to link the task to, by id.
    pub person_ids: Option<Vec<String>>,
    /// Companies to link the task to, by id.
    pub company_ids: Option<Vec<String>>,
    /// Opportunities to link the task to, by id.
    pub opportunity_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListMembersParams {
    /// Members per page, 1 to 200. Default: 20.
    pub limit: Option<u32>,
    /// next_cursor from the previous page.
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RecordActivityParams {
    /// people, companies or opportunities.
    pub object: String,
    /// The record's id.
    pub id: String,
    /// Notes and tasks each, newest first, 1 to 200. Default: 20.
    pub limit: Option<u32>,
}

/// The morph field of noteTargets and taskTargets that points at records of `object`.
fn target_field(object: &str) -> error::Result<&'static str> {
    match object.trim() {
        "people" | "person" => Ok("targetPerson"),
        "companies" | "company" => Ok("targetCompany"),
        "opportunities" | "opportunity" => Ok("targetOpportunity"),
        _ => Err(TwentyError::invalid(
            "object must be people, companies or opportunities",
        )),
    }
}

/// `extra` fields with the typed ones laid over them. `extra` may not carry an id.
fn merge(
    extra: Option<&Map<String, Value>>,
    typed: Vec<(&str, Option<Value>)>,
) -> error::Result<Value> {
    let mut out = extra.cloned().unwrap_or_default();
    if out.contains_key("id") {
        return Err(TwentyError::invalid("extra must not contain id"));
    }
    for (k, v) in typed {
        if let Some(v) = v {
            out.insert(k.to_string(), v);
        }
    }
    Ok(Value::Object(out))
}

fn link(url: Option<String>) -> Option<Value> {
    url.map(|u| json!({"primaryLinkUrl": u}))
}

fn body_arg(v: Option<&str>) -> error::Result<Option<Value>> {
    let Some(b) = v.map(str::trim).filter(|b| !b.is_empty()) else {
        return Ok(None);
    };
    if b.chars().count() > MAX_BODY_CHARS {
        return Err(TwentyError::invalid(format!(
            "body is limited to {MAX_BODY_CHARS} characters"
        )));
    }
    Ok(Some(json!({"markdown": b})))
}

/// An amount as Twenty's currency composite: micros and a currency code.
pub(crate) fn money(amount: f64, currency: &str) -> error::Result<Value> {
    if !amount.is_finite() || amount.abs() > MAX_AMOUNT {
        return Err(TwentyError::invalid(
            "amount must be a number below 1,000,000,000,000",
        ));
    }
    Ok(json!({
        "amountMicros": (amount * 1_000_000.0).round() as i64,
        "currencyCode": currency,
    }))
}

fn opt_id(name: &str, v: Option<&str>) -> error::Result<Option<Value>> {
    v.map(|raw| id_arg(name, raw).map(Value::from)).transpose()
}

fn text(name: &str, v: Option<&str>) -> error::Result<Option<Value>> {
    Ok(text_arg(name, v)?.map(Value::from))
}

/// Links targets of a new note or task, one request each. Failures are reported, not raised, so
/// the caller does not create the note again.
async fn link_targets(
    c: &super::Caller,
    targets_object: &str,
    owner_field: &str,
    owner_id: &str,
    targets: &[(&str, Vec<String>)],
) -> (Vec<Value>, Vec<Value>) {
    let mut linked = Vec::new();
    let mut failed = Vec::new();
    for (field, ids) in targets {
        for id in ids {
            let mut body = Map::new();
            body.insert(owner_field.to_string(), Value::from(owner_id));
            body.insert(format!("{field}Id"), Value::from(id.as_str()));
            let body = Value::Object(body);
            match c
                .send(
                    Method::POST,
                    "link_target",
                    &[targets_object],
                    &[],
                    Some(&body),
                )
                .await
            {
                Ok(_) => linked.push(json!({"to": field, "id": id})),
                Err(e) => failed
                    .push(json!({"to": field, "id": id, "error": format!("{}: {e}", e.kind())})),
            }
        }
    }
    (linked, failed)
}

fn targets(
    p_people: Option<&[String]>,
    p_companies: Option<&[String]>,
    p_opps: Option<&[String]>,
) -> error::Result<Vec<(&'static str, Vec<String>)>> {
    Ok(vec![
        ("targetPerson", ids_arg("person_ids", p_people)?),
        ("targetCompany", ids_arg("company_ids", p_companies)?),
        ("targetOpportunity", ids_arg("opportunity_ids", p_opps)?),
    ])
}

/// A note or task for output: the Markdown body without the editor's block JSON.
fn activity(v: &Value) -> Value {
    let mut v = record(v);
    if let Some(body) = v.get_mut("bodyV2").and_then(Value::as_object_mut) {
        body.remove("blocknote");
    }
    v
}

fn with_links(created: Value, linked: Vec<Value>, failed: Vec<Value>) -> Value {
    let mut out = json!({"record": activity(&created), "linked": linked});
    if !failed.is_empty() {
        out["link_errors"] = Value::Array(failed);
        out["note"] = Value::from(
            "the record was created but some links failed; do not create it again, link the rest with create_record",
        );
    }
    out
}

#[tool_router(router = crm_router, vis = "pub(crate)")]
impl CrmServer {
    #[tool(
        description = "Create a person (contact) from plain fields: name, email, phone, job title, city, company and LinkedIn URL, plus any other fields in extra. Search first so you do not create a duplicate. Returns the new person with its id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_person(
        &self,
        Parameters(p): Parameters<CreatePersonParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_person", async {
            let c = self.caller(&parts)?;
            let mut name = json!({"firstName": required_text("first_name", &p.first_name)?});
            if let Some(last) = text_arg("last_name", p.last_name.as_deref())? {
                name["lastName"] = Value::from(last);
            }
            let phone = text_arg("phone", p.phone.as_deref())?.map(|n| {
                let mut v = json!({"primaryPhoneNumber": n});
                if let Ok(Some(code)) =
                    text_arg("phone_calling_code", p.phone_calling_code.as_deref())
                {
                    v["primaryPhoneCallingCode"] = Value::from(code);
                }
                v
            });
            let data = merge(
                p.extra.as_ref(),
                vec![
                    ("name", Some(name)),
                    (
                        "emails",
                        text_arg("email", p.email.as_deref())?.map(|e| json!({"primaryEmail": e})),
                    ),
                    ("phones", phone),
                    ("jobTitle", text("job_title", p.job_title.as_deref())?),
                    ("city", text("city", p.city.as_deref())?),
                    ("companyId", opt_id("company_id", p.company_id.as_deref())?),
                    (
                        "linkedinLink",
                        link(text_arg("linkedin_url", p.linkedin_url.as_deref())?),
                    ),
                ],
            )?;
            Ok(json_result(&c.create("people", data).await?))
        })
        .await
    }

    #[tool(
        description = "Create a company from plain fields: name, website, employees, LinkedIn URL, address and account owner, plus any other fields in extra. Search first so you do not create a duplicate. Returns the new company with its id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_company(
        &self,
        Parameters(p): Parameters<CreateCompanyParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_company", async {
            let c = self.caller(&parts)?;
            let mut address = Map::new();
            for (k, v) in [
                ("addressStreet1", &p.address_street),
                ("addressCity", &p.city),
                ("addressPostcode", &p.postcode),
                ("addressCountry", &p.country),
            ] {
                if let Some(t) = text_arg(k, v.as_deref())? {
                    address.insert(k.to_string(), Value::from(t));
                }
            }
            let data = merge(
                p.extra.as_ref(),
                vec![
                    ("name", Some(Value::from(required_text("name", &p.name)?))),
                    ("domainName", link(text_arg("domain", p.domain.as_deref())?)),
                    ("employees", p.employees.map(Value::from)),
                    (
                        "linkedinLink",
                        link(text_arg("linkedin_url", p.linkedin_url.as_deref())?),
                    ),
                    (
                        "address",
                        (!address.is_empty()).then_some(Value::Object(address)),
                    ),
                    (
                        "accountOwnerId",
                        opt_id("account_owner_id", p.account_owner_id.as_deref())?,
                    ),
                ],
            )?;
            Ok(json_result(&c.create("companies", data).await?))
        })
        .await
    }

    #[tool(
        description = "Create an opportunity (deal) from plain fields: name, amount and currency, stage, close date, company and point of contact, plus any other fields in extra. Returns the new opportunity with its id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_opportunity(
        &self,
        Parameters(p): Parameters<CreateOpportunityParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_opportunity", async {
            let c = self.caller(&parts)?;
            let currency = match p.currency_code.as_deref() {
                Some(raw) => currency_code(raw).ok_or_else(|| {
                    TwentyError::invalid(
                        "currency_code must be a three-letter ISO 4217 code such as GBP",
                    )
                })?,
                None => self.default_currency.clone(),
            };
            let amount = p.amount.map(|a| money(a, &currency)).transpose()?;
            let stage = text_arg("stage", p.stage.as_deref())?
                .map(|s| Value::from(s.to_ascii_uppercase().replace([' ', '-'], "_")));
            let data = merge(
                p.extra.as_ref(),
                vec![
                    ("name", Some(Value::from(required_text("name", &p.name)?))),
                    ("amount", amount),
                    ("stage", stage),
                    ("closeDate", text("close_date", p.close_date.as_deref())?),
                    ("companyId", opt_id("company_id", p.company_id.as_deref())?),
                    (
                        "pointOfContactId",
                        opt_id("point_of_contact_id", p.point_of_contact_id.as_deref())?,
                    ),
                ],
            )?;
            Ok(json_result(&c.create("opportunities", data).await?))
        })
        .await
    }

    #[tool(
        description = "Create a note with a Markdown body and link it to people, companies and opportunities. Returns the note and the links made.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_note(
        &self,
        Parameters(p): Parameters<CreateNoteParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_note", async {
            let c = self.caller(&parts)?;
            let targets = targets(
                p.person_ids.as_deref(),
                p.company_ids.as_deref(),
                p.opportunity_ids.as_deref(),
            )?;
            let data = merge(
                None,
                vec![
                    (
                        "title",
                        Some(Value::from(required_text("title", &p.title)?)),
                    ),
                    ("bodyV2", body_arg(p.body.as_deref())?),
                ],
            )?;
            let note = c.create("notes", data).await?;
            let id = note
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let (linked, failed) = link_targets(&c, "noteTargets", "noteId", &id, &targets).await;
            Ok(json_result(&with_links(note, linked, failed)))
        })
        .await
    }

    #[tool(
        description = "Create a task with an optional due date, status, assignee and Markdown body, linked to people, companies and opportunities. Returns the task and the links made.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn create_task(
        &self,
        Parameters(p): Parameters<CreateTaskParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("create_task", async {
            let c = self.caller(&parts)?;
            let targets = targets(
                p.person_ids.as_deref(),
                p.company_ids.as_deref(),
                p.opportunity_ids.as_deref(),
            )?;
            let status = match text_arg("status", p.status.as_deref())? {
                None => None,
                Some(s) => {
                    let s = s.to_ascii_uppercase().replace([' ', '-'], "_");
                    if !matches!(s.as_str(), "TODO" | "IN_PROGRESS" | "DONE") {
                        return Err(TwentyError::invalid(
                            "status must be TODO, IN_PROGRESS or DONE",
                        ));
                    }
                    Some(Value::from(s))
                }
            };
            let data = merge(
                None,
                vec![
                    (
                        "title",
                        Some(Value::from(required_text("title", &p.title)?)),
                    ),
                    ("bodyV2", body_arg(p.body.as_deref())?),
                    ("dueAt", text("due_at", p.due_at.as_deref())?),
                    ("status", status),
                    (
                        "assigneeId",
                        opt_id("assignee_id", p.assignee_id.as_deref())?,
                    ),
                ],
            )?;
            let task = c.create("tasks", data).await?;
            let id = task
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let (linked, failed) = link_targets(&c, "taskTargets", "taskId", &id, &targets).await;
            Ok(json_result(&with_links(task, linked, failed)))
        })
        .await
    }

    #[tool(
        description = "List the workspace's members (the people who use Twenty), with their ids for assigning tasks and owning accounts.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_workspace_members(
        &self,
        Parameters(p): Parameters<ListMembersParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("list_workspace_members", async {
            let c = self.caller(&parts)?;
            let mut q = vec![(
                "limit",
                limit_arg(p.limit, DEFAULT_LIMIT, MAX_LIMIT)?.to_string(),
            )];
            if let Some(cursor) = p.cursor.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                q.push(("starting_after", cursor.to_string()));
            }
            Ok(json_result(&c.list_page("workspaceMembers", &q).await?))
        })
        .await
    }

    #[tool(
        description = "List the notes and tasks linked to one person, company or opportunity, newest first, with their Markdown bodies, due dates and status.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_record_activity(
        &self,
        Parameters(p): Parameters<RecordActivityParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        run("list_record_activity", async {
            let c = self.caller(&parts)?;
            let field = target_field(&p.object)?;
            let id = id_arg("id", &p.id)?;
            let limit = limit_arg(p.limit, DEFAULT_LIMIT, MAX_LIMIT)?;
            let mut out = Map::new();
            for (targets, inner, key) in [
                ("noteTargets", "note", "notes"),
                ("taskTargets", "task", "tasks"),
            ] {
                let q = vec![
                    ("filter", format!("{field}Id[eq]:{id}")),
                    ("order_by", "createdAt[DescNullsLast]".to_string()),
                    ("limit", limit.to_string()),
                    ("depth", "1".to_string()),
                ];
                let found = c.get("record_activity", &[targets], &q).await?;
                let items: Vec<Value> = unwrap_data(&found)
                    .as_array()
                    .map(|links| {
                        links
                            .iter()
                            .filter_map(|l| l.get(inner))
                            .map(activity)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut section = json!({"count": items.len(), "items": items});
                if found
                    .pointer("/pageInfo/hasNextPage")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    section["more"] = Value::Bool(true);
                }
                out.insert(key.to_string(), section);
            }
            Ok(json_result(&Value::Object(out)))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_become_micros() {
        assert_eq!(
            money(12500.5, "GBP").unwrap(),
            json!({"amountMicros": 12_500_500_000_i64, "currencyCode": "GBP"})
        );
        assert!(money(f64::NAN, "GBP").is_err());
        assert!(money(2e12, "GBP").is_err());
    }

    #[test]
    fn typed_fields_override_extra_and_ids_are_refused() {
        let extra = json!({"name": "x", "tier": "A"});
        let v = merge(
            extra.as_object(),
            vec![("name", Some(json!("Acme"))), ("city", None)],
        )
        .unwrap();
        assert_eq!(v, json!({"name": "Acme", "tier": "A"}));
        let with_id = json!({"id": "x"});
        assert!(merge(with_id.as_object(), vec![]).is_err());
    }

    #[test]
    fn activity_targets_are_the_standard_objects() {
        assert_eq!(target_field("people").unwrap(), "targetPerson");
        assert_eq!(target_field("company").unwrap(), "targetCompany");
        assert!(target_field("notes").is_err());
    }

    #[test]
    fn activity_drops_the_editor_json() {
        let note = json!({"id": "1", "bodyV2": {"markdown": "hi", "blocknote": "[{...}]"}});
        assert_eq!(
            activity(&note),
            json!({"id": "1", "bodyV2": {"markdown": "hi"}})
        );
    }
}
