//! In-memory Twenty REST API shaped like the real one: bearer auth, `/rest/{object}` CRUD with
//! `{"data": {...}}` envelopes, cursor paging, soft delete and restore, `/rest/batch/{object}`
//! and `/rest/metadata/objects`. Filters are matched loosely: a record matches when it contains
//! every `%word%` and every `[eq]:` value of the filter. Every request is logged for assertions;
//! failures can be injected.
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use url::Url;

/// A JWT-shaped test key: three base64url parts, about as long as a real Twenty key (over the
/// core's default credential limit of 256 bytes).
pub const API_KEY: &str = "eyJhbGciOiJIUzI1NiJ9.eyJ0ZXN0IjoidGVzdCJ9AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA.test-signature_abc";

#[derive(Debug, Clone)]
pub struct Logged {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub authorization: Option<String>,
    pub body: Option<Value>,
}

impl Logged {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Injected {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(&'static str, String)>,
}

impl Injected {
    pub fn json(status: u16, body: Value) -> Self {
        Injected {
            status,
            body: body.to_string(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

#[derive(Debug, Default)]
pub struct FakeState {
    /// Records by object plural name, in creation order.
    pub records: BTreeMap<String, Vec<Value>>,
    /// Soft-deleted records by object.
    pub trash: BTreeMap<String, Vec<Value>>,
    pub log: Vec<Logged>,
    pub inject: VecDeque<Injected>,
    pub next_id: u64,
    /// Objects the key's role may not read.
    pub forbidden: Vec<String>,
}

pub const OBJECTS: &[&str] = &[
    "people",
    "companies",
    "opportunities",
    "notes",
    "tasks",
    "noteTargets",
    "taskTargets",
    "workspaceMembers",
];

pub fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

impl FakeState {
    /// A small workspace: two companies, three people, an opportunity, a note and a member.
    pub fn workspace() -> Self {
        let mut s = FakeState {
            next_id: 100,
            ..Default::default()
        };
        for o in OBJECTS {
            s.records.insert(o.to_string(), Vec::new());
        }
        let acme = uuid(1);
        let globex = uuid(2);
        s.put("companies", json!({"id": acme, "name": "Acme Widgets", "domainName": {"primaryLinkUrl": "https://acme.example", "secondaryLinks": []}, "employees": 50, "searchVector": "x"}));
        s.put("companies", json!({"id": globex, "name": "Globex", "domainName": {"primaryLinkUrl": "https://globex.example"}, "employees": 900}));
        s.put("people", json!({"id": uuid(11), "name": {"firstName": "Ann", "lastName": "Lee"}, "emails": {"primaryEmail": "ann@acme.example", "additionalEmails": []}, "jobTitle": "CTO", "companyId": acme, "city": ""}));
        s.put("people", json!({"id": uuid(12), "name": {"firstName": "Bob", "lastName": "Stone"}, "emails": {"primaryEmail": "bob@acme.example"}, "companyId": acme}));
        s.put("people", json!({"id": uuid(13), "name": {"firstName": "Ann", "lastName": "Smith"}, "emails": {"primaryEmail": "ann@globex.example"}, "companyId": globex}));
        s.put("opportunities", json!({"id": uuid(21), "name": "Acme renewal", "stage": "PROPOSAL", "amount": {"amountMicros": 5_000_000_000_i64, "currencyCode": "GBP"}, "companyId": acme}));
        s.put("notes", json!({"id": uuid(31), "title": "Kick-off call", "bodyV2": {"markdown": "Went well", "blocknote": "[]"}, "createdAt": "2026-01-02T00:00:00Z"}));
        s.put(
            "noteTargets",
            json!({"id": uuid(41), "noteId": uuid(31), "targetCompanyId": acme}),
        );
        s.put("workspaceMembers", json!({"id": uuid(51), "name": {"firstName": "Sam", "lastName": "Owner"}, "userEmail": "sam@example.com"}));
        s
    }

    fn put(&mut self, object: &str, v: Value) {
        self.records.entry(object.to_string()).or_default().push(v);
    }

    fn new_id(&mut self) -> String {
        self.next_id += 1;
        uuid(self.next_id)
    }
}

pub struct FakeTwenty {
    pub url: Url,
    state: Arc<Mutex<FakeState>>,
}

impl FakeTwenty {
    pub async fn start(state: FakeState) -> FakeTwenty {
        let state = Arc::new(Mutex::new(state));
        let app = axum::Router::new()
            .fallback(handle)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeTwenty {
            url: Url::parse(&format!("http://{addr}/")).unwrap(),
            state,
        }
    }

    pub fn state(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    pub fn requests(&self) -> Vec<Logged> {
        self.state().log.clone()
    }

    pub fn inject(&self, response: Injected) {
        self.state().inject.push_back(response);
    }

    pub fn records(&self, object: &str) -> Vec<Value> {
        self.state()
            .records
            .get(object)
            .cloned()
            .unwrap_or_default()
    }
}

fn reply(status: u16, body: Value) -> Response {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn bad_request(msg: &str) -> Response {
    reply(
        400,
        json!({"statusCode": 400, "error": "BadRequestException", "messages": [msg]}),
    )
}

fn singular(object: &str) -> String {
    match object {
        "people" => "Person".to_string(),
        "companies" => "Company".to_string(),
        "opportunities" => "Opportunity".to_string(),
        o => {
            let mut c = o.chars();
            let first = c.next().map(|f| f.to_ascii_uppercase()).unwrap_or('X');
            format!("{first}{}", c.as_str().trim_end_matches('s'))
        }
    }
}

/// The loose filter match: every `%word%` (case-insensitive) and every `[eq]:value` must appear
/// in the record's JSON.
fn matches(record: &Value, filter: &str) -> bool {
    let text = record.to_string().to_lowercase();
    let words = filter.split('%').skip(1).step_by(2).map(str::to_lowercase);
    let eqs = filter.split("[eq]:").skip(1).map(|rest| {
        rest.split([',', ')'])
            .next()
            .unwrap_or_default()
            .to_lowercase()
    });
    let mut all = words.chain(eqs);
    // An `or(...)` of fields is satisfied by any; with the loose match every term must appear,
    // except that the same word repeated across fields counts once.
    all.all(|w| text.contains(&w))
}

fn embed(state: &FakeState, mut r: Value) -> Value {
    for (key, object) in [
        ("noteId", "notes"),
        ("taskId", "tasks"),
        ("companyId", "companies"),
    ] {
        let Some(id) = r.get(key).and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        let found = state
            .records
            .get(object)
            .and_then(|rs| rs.iter().find(|x| x["id"] == id.as_str()))
            .cloned();
        if let Some(found) = found {
            r[key.trim_end_matches("Id")] = found;
        }
    }
    r
}

async fn handle(
    State(state): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let query: Vec<(String, String)> = uri
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    let body: Option<Value> = serde_json::from_slice(&body).ok();
    let mut st = state.lock().unwrap();
    st.log.push(Logged {
        method: method.to_string(),
        path: uri.path().to_string(),
        query: query.clone(),
        authorization: headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        body: body.clone(),
    });
    if headers.get("authorization").and_then(|v| v.to_str().ok())
        != Some(&format!("Bearer {API_KEY}"))
    {
        return reply(401, json!({"statusCode": 401, "message": "Token invalid"}));
    }
    if let Some(inj) = st.inject.pop_front() {
        let mut b = Response::builder()
            .status(StatusCode::from_u16(inj.status).unwrap())
            .header("content-type", "application/json");
        for (k, v) in inj.headers {
            b = b.header(k, v);
        }
        return b.body(Body::from(inj.body)).unwrap();
    }
    let param = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let segs: Vec<String> = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .map(|s| {
            url::form_urlencoded::parse(s.as_bytes())
                .map(|(k, _)| k.into_owned())
                .next()
                .unwrap_or_default()
        })
        .collect();
    let segs: Vec<&str> = segs.iter().map(String::as_str).collect();
    match (method.as_str(), segs.as_slice()) {
        ("GET", ["rest", "metadata", "objects"]) => reply(200, metadata()),
        ("GET", ["rest", "metadata", "objects", "obj-opps"]) => reply(
            200,
            json!({"data": {"object": {"id": "obj-opps", "namePlural": "opportunities", "fields": [
                {"name": "stage", "type": "SELECT", "label": "Stage", "isActive": true, "isNullable": false,
                 "options": [{"value": "NEW"}, {"value": "PROPOSAL"}, {"value": "CUSTOMER"}]},
                {"name": "amount", "type": "CURRENCY", "label": "Amount", "isActive": true, "isNullable": true}
            ]}}}),
        ),
        ("GET", ["rest", "metadata", "objects", _]) => {
            reply(200, json!({"data": {"object": {"fields": []}}}))
        }
        ("POST", ["rest", "batch", object]) => {
            let Some(Value::Array(items)) = body else {
                return bad_request("body must be an array");
            };
            let mut created = Vec::new();
            for mut item in items {
                item["id"] = Value::from(st.new_id());
                st.put(object, item.clone());
                created.push(item);
            }
            reply(
                200,
                json!({"data": {format!("create{}", singular(object)) + "s": created}}),
            )
        }
        (_, ["rest", object, ..]) if !st.records.contains_key(*object) => {
            bad_request(&format!("object '{object}' not found. eg: companies"))
        }
        (_, ["rest", object, ..]) if st.forbidden.iter().any(|f| f == object) => reply(
            403,
            json!({"statusCode": 403, "message": "Entity not readable by this role"}),
        ),
        ("GET", ["rest", object]) => {
            let limit: usize = param("limit").and_then(|l| l.parse().ok()).unwrap_or(60);
            let filter = param("filter").unwrap_or_default();
            let all: Vec<Value> = st.records[*object]
                .iter()
                .filter(|r| matches(r, &filter))
                .cloned()
                .collect();
            let start = match param("starting_after") {
                Some(c) => all
                    .iter()
                    .position(|r| r["id"] == c.as_str())
                    .map_or(all.len(), |p| p + 1),
                None => 0,
            };
            let mut page: Vec<Value> = all.iter().skip(start).take(limit).cloned().collect();
            if param("depth").as_deref() == Some("1") {
                page = page.into_iter().map(|r| embed(&st, r)).collect();
            }
            let has_next = start + page.len() < all.len();
            let end = page.last().map(|r| r["id"].clone()).unwrap_or(Value::Null);
            reply(
                200,
                json!({
                    "data": {*object: page},
                    "totalCount": all.len(),
                    "pageInfo": {"hasNextPage": has_next, "hasPreviousPage": start > 0, "startCursor": null, "endCursor": end}
                }),
            )
        }
        ("GET", ["rest", object, id]) => {
            match st.records[*object].iter().find(|r| r["id"] == *id).cloned() {
                Some(r) => {
                    let r = if param("depth").as_deref() == Some("1") {
                        embed(&st, r)
                    } else {
                        r
                    };
                    reply(200, json!({"data": {singular(object).to_lowercase(): r}}))
                }
                None => reply(
                    404,
                    json!({"statusCode": 404, "message": "Record not found"}),
                ),
            }
        }
        ("POST", ["rest", object]) => {
            let Some(Value::Object(mut fields)) = body else {
                return bad_request("body must be an object");
            };
            let id = st.new_id();
            fields.insert("id".into(), Value::from(id));
            let r = Value::Object(fields);
            st.put(object, r.clone());
            reply(
                201,
                json!({"data": {format!("create{}", singular(object)): r}}),
            )
        }
        ("PATCH", ["rest", object, id, "restore"]) => {
            let trash = st.trash.entry(object.to_string()).or_default();
            match trash.iter().position(|r| r["id"] == *id) {
                Some(p) => {
                    let r = trash.remove(p);
                    st.put(object, r.clone());
                    reply(
                        200,
                        json!({"data": {format!("restore{}", singular(object)): r}}),
                    )
                }
                None => reply(
                    404,
                    json!({"statusCode": 404, "message": "Record not found"}),
                ),
            }
        }
        ("PATCH", ["rest", object, id]) => {
            let Some(Value::Object(fields)) = body else {
                return bad_request("body must be an object");
            };
            let records = st.records.get_mut(*object).unwrap();
            match records.iter_mut().find(|r| r["id"] == *id) {
                Some(r) => {
                    let map: &mut Map<String, Value> = r.as_object_mut().unwrap();
                    map.extend(fields);
                    let r = r.clone();
                    reply(
                        200,
                        json!({"data": {format!("update{}", singular(object)): r}}),
                    )
                }
                None => reply(
                    404,
                    json!({"statusCode": 404, "message": "Record not found"}),
                ),
            }
        }
        ("DELETE", ["rest", object, id]) => {
            let records = st.records.get_mut(*object).unwrap();
            let Some(p) = records.iter().position(|r| r["id"] == *id) else {
                return reply(
                    404,
                    json!({"statusCode": 404, "message": "Record not found"}),
                );
            };
            let r = records.remove(p);
            if param("soft_delete").as_deref() == Some("true") {
                st.trash.entry(object.to_string()).or_default().push(r);
            }
            reply(
                200,
                json!({"data": {format!("delete{}", singular(object)): {"id": id}}}),
            )
        }
        _ => reply(404, json!({"statusCode": 404, "message": "Cannot route"})).into_response(),
    }
}

fn metadata() -> Value {
    json!({
        "data": {"objects": [
            {
                "id": "obj-people", "nameSingular": "person", "namePlural": "people",
                "labelSingular": "Person", "labelPlural": "People", "isActive": true, "isSystem": false, "isCustom": false,
                "fields": [
                    {"name": "name", "type": "FULL_NAME", "label": "Name", "isActive": true, "isSystem": false, "isNullable": true},
                    {"name": "emails", "type": "EMAILS", "label": "Emails", "isActive": true, "isSystem": false, "isNullable": true},
                    {"name": "company", "type": "RELATION", "label": "Company", "isActive": true, "isSystem": false, "isNullable": true,
                     "settings": {"relationType": "MANY_TO_ONE"}, "relation": {"targetObjectMetadata": {"nameSingular": "company"}}},
                    {"name": "position", "type": "POSITION", "label": "Position", "isActive": true, "isSystem": true},
                    {"name": "legacy", "type": "TEXT", "label": "Old", "isActive": false, "isSystem": false}
                ]
            },
            {
                "id": "obj-opps", "nameSingular": "opportunity", "namePlural": "opportunities",
                "labelSingular": "Opportunity", "labelPlural": "Opportunities", "isActive": true, "isSystem": false, "isCustom": false
            },
            {
                "id": "obj-pets", "nameSingular": "pet", "namePlural": "pets",
                "labelSingular": "Pet", "labelPlural": "Pets", "isActive": true, "isSystem": false, "isCustom": true,
                "fields": []
            },
            {
                "id": "obj-blocklist", "nameSingular": "blocklist", "namePlural": "blocklists",
                "labelSingular": "Blocklist", "labelPlural": "Blocklists", "isActive": true, "isSystem": true, "isCustom": false
            }
        ]},
        "totalCount": 4,
        "pageInfo": {"hasNextPage": false}
    })
}
