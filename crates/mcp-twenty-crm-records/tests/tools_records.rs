mod support;
use serde_json::{Value, json};
use support::fake_twenty::{API_KEY, FakeState, FakeTwenty, Injected, uuid};
use support::tools::{auth, auth_with, error_text, json, no_auth, params, server};

async fn fake() -> FakeTwenty {
    FakeTwenty::start(FakeState::workspace()).await
}

fn ids(v: &Value) -> Vec<&str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn list_records_sends_the_query_and_pages_with_cursors() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.list_records(
            params(json!({
                "object": "people",
                "limit": 2,
                "order_by": "createdAt[DescNullsLast]",
                "fields": ["name", "emails"],
                "depth": 0
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["object"], "people");
    assert_eq!(v["total"], 3);
    assert_eq!(v["count"], 2);
    assert_eq!(ids(&v), vec![uuid(11), uuid(12)]);
    assert_eq!(v["next_cursor"], uuid(12));
    let log = fake.requests();
    assert_eq!(log[0].method, "GET");
    assert_eq!(log[0].path, "/rest/people");
    assert_eq!(log[0].param("limit"), Some("2"));
    assert_eq!(log[0].param("order_by"), Some("createdAt[DescNullsLast]"));
    assert_eq!(log[0].param("fields"), Some("name,emails"));
    assert_eq!(log[0].param("depth"), Some("0"));
    assert_eq!(
        log[0].authorization.as_deref(),
        Some(format!("Bearer {API_KEY}").as_str())
    );

    let v = json(
        &s.list_records(
            params(json!({"object": "people", "limit": 2, "cursor": uuid(12)})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(ids(&v), vec![uuid(13)]);
    assert!(v.get("next_cursor").is_none(), "{v}");
    assert_eq!(
        fake.requests()[1].param("starting_after"),
        Some(uuid(12).as_str())
    );
}

#[tokio::test]
async fn list_records_passes_the_filter_through_and_drops_empty_values() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_records(
                params(json!({"object": "people", "filter": format!("companyId[eq]:{}", uuid(2))})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(ids(&v), vec![uuid(13)]);
    assert_eq!(
        fake.requests()[0].param("filter"),
        Some(format!("companyId[eq]:{}", uuid(2)).as_str())
    );
    let first = json(
        &server(&fake)
            .list_records(params(json!({"object": "people", "limit": 1})), auth())
            .await
            .unwrap(),
    );
    let person = &first["items"][0];
    assert!(person.get("city").is_none(), "{person}");
    assert!(
        person["emails"].get("additionalEmails").is_none(),
        "{person}"
    );
}

#[tokio::test]
async fn bad_arguments_are_refused_before_calling_twenty() {
    let fake = fake().await;
    let s = server(&fake);
    for args in [
        json!({"object": "../people"}),
        json!({"object": "people", "limit": 500}),
        json!({"object": "people", "order_by": "x&y"}),
        json!({"object": "people", "depth": 3}),
    ] {
        let e = error_text(&s.list_records(params(args.clone()), auth()).await.unwrap());
        assert!(e.starts_with("validation: "), "{args}: {e}");
    }
    let e = error_text(
        &s.get_record(
            params(json!({"object": "people", "id": "not-an-id"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.contains("UUID"), "{e}");
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn get_record_unwraps_the_envelope_and_names_what_was_missing() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.get_record(params(json!({"object": "people", "id": uuid(11)})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["name"]["firstName"], "Ann");
    assert_eq!(v["company"]["name"], "Acme Widgets");
    assert!(v["company"].get("searchVector").is_none(), "{v}");
    assert_eq!(fake.requests()[0].param("depth"), Some("1"));

    let e = error_text(
        &s.get_record(params(json!({"object": "people", "id": uuid(999)})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("not_found: no people record with id"), "{e}");
}

#[tokio::test]
async fn search_matches_every_word_across_objects() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .search_records(params(json!({"query": "ann lee"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(ids(&v["people"]), vec![uuid(11)]);
    assert_eq!(v["companies"]["count"], 0);
    assert_eq!(v["opportunities"]["count"], 0);
    let log = fake.requests();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].path, "/rest/people");
    assert_eq!(
        log[0].param("filter"),
        Some(
            "and(or(name.firstName[ilike]:%ann%,name.lastName[ilike]:%ann%,emails.primaryEmail[ilike]:%ann%),or(name.firstName[ilike]:%lee%,name.lastName[ilike]:%lee%,emails.primaryEmail[ilike]:%lee%))"
        )
    );
    assert_eq!(log[0].param("limit"), Some("5"));
}

#[tokio::test]
async fn search_reports_one_object_failing_and_keeps_the_rest() {
    let fake = fake().await;
    fake.state().forbidden.push("opportunities".into());
    let v = json(
        &server(&fake)
            .search_records(
                params(json!({"query": "acme", "objects": ["companies", "opportunities"]})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(ids(&v["companies"]), vec![uuid(1)]);
    let e = v["opportunities"]["error"].as_str().unwrap();
    assert!(e.starts_with("permission: "), "{e}");
}

#[tokio::test]
async fn create_update_delete_and_restore_round_trip() {
    let fake = fake().await;
    let s = server(&fake);
    let created = json(
        &s.create_record(
            params(json!({"object": "companies", "data": {"name": "Initech", "employees": 10}})),
            auth(),
        )
        .await
        .unwrap(),
    );
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "Initech");
    let log = fake.requests();
    assert_eq!(log[0].method, "POST");
    assert_eq!(log[0].path, "/rest/companies");
    assert_eq!(
        log[0].body,
        Some(json!({"name": "Initech", "employees": 10}))
    );

    let updated = json(
        &s.update_record(
            params(json!({"object": "companies", "id": id, "data": {"employees": 12}})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(updated["employees"], 12);
    assert_eq!(fake.requests()[1].method, "PATCH");

    let deleted = json(
        &s.delete_record(params(json!({"object": "companies", "id": id})), auth())
            .await
            .unwrap(),
    );
    assert!(deleted["deleted"].as_str().unwrap().contains("trash"));
    assert_eq!(fake.requests()[2].param("soft_delete"), Some("true"));
    assert!(
        fake.records("companies")
            .iter()
            .all(|r| r["id"] != id.as_str())
    );

    let restored = json(
        &s.restore_record(params(json!({"object": "companies", "id": id})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(restored["name"], "Initech");
    assert_eq!(
        fake.requests()[3].path,
        format!("/rest/companies/{id}/restore")
    );
}

#[tokio::test]
async fn permanent_delete_says_so_to_twenty() {
    let fake = fake().await;
    json(
        &server(&fake)
            .delete_record(
                params(json!({"object": "people", "id": uuid(12), "permanent": true})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(fake.requests()[0].param("soft_delete"), Some("false"));
}

#[tokio::test]
async fn writes_refuse_an_id_in_data_and_empty_data() {
    let fake = fake().await;
    let s = server(&fake);
    for data in [json!({}), json!({"id": uuid(1), "name": "x"})] {
        let e = error_text(
            &s.create_record(params(json!({"object": "companies", "data": data})), auth())
                .await
                .unwrap(),
        );
        assert!(e.starts_with("validation: "), "{e}");
    }
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn batch_create_posts_to_the_batch_endpoint() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.create_records(
            params(json!({"object": "companies", "records": [{"name": "A"}, {"name": "B"}]})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["count"], 2);
    assert_eq!(fake.requests()[0].path, "/rest/batch/companies");
    let many: Vec<Value> = (0..61).map(|i| json!({"name": format!("c{i}")})).collect();
    let e = error_text(
        &s.create_records(
            params(json!({"object": "companies", "records": many})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.contains("1 to 60"), "{e}");
}

#[tokio::test]
async fn upstream_errors_become_kinded_tool_errors() {
    let fake = fake().await;
    let s = server(&fake);
    let list = || params(json!({"object": "people"}));

    let e = error_text(
        &s.list_records(list(), auth_with("a.b.wrong-key"))
            .await
            .unwrap(),
    );
    assert!(e.starts_with("auth: Twenty rejected your API key"), "{e}");

    fake.inject(Injected::json(429, json!({"message": "Too many"})).header("retry-after", "30"));
    let e = error_text(&s.list_records(list(), auth()).await.unwrap());
    assert!(e.starts_with("rate_limited: "), "{e}");
    assert!(e.contains("30 seconds"), "{e}");

    let e = error_text(
        &s.list_records(params(json!({"object": "persons"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("validation: "), "{e}");
    assert!(e.contains("object 'persons' not found"), "{e}");

    fake.state().forbidden.push("people".into());
    let e = error_text(&s.list_records(list(), auth()).await.unwrap());
    assert!(e.starts_with("permission: "), "{e}");

    let e = error_text(&s.list_records(list(), no_auth()).await.unwrap());
    assert!(e.starts_with("auth: "), "{e}");
}

#[tokio::test]
async fn reads_are_retried_once_after_a_server_error_but_writes_are_not() {
    let fake = fake().await;
    let s = server(&fake);
    fake.inject(Injected::json(502, json!({})));
    json(
        &s.list_records(params(json!({"object": "people"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(fake.requests().len(), 2);

    fake.inject(Injected::json(502, json!({})));
    let e = error_text(
        &s.create_record(
            params(json!({"object": "companies", "data": {"name": "X"}})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.starts_with("upstream: "), "{e}");
    assert_eq!(fake.requests().len(), 3);
}

#[tokio::test]
async fn metadata_lists_objects_and_describes_fields() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(&s.list_objects(params(json!({})), auth()).await.unwrap());
    let names: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["opportunities", "people", "pets"]);
    assert_eq!(v["items"][2]["custom"], true);
    let v = json(
        &s.list_objects(params(json!({"include_system": true})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["count"], 4);

    let v = json(
        &s.describe_object(params(json!({"object": "person"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["name"], "people");
    let fields: Vec<&str> = v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(fields, vec!["company", "emails", "name"]);
    assert_eq!(v["fields"][0]["relation_to"], "company");

    let v = json(
        &s.describe_object(params(json!({"object": "opportunities"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(
        v["fields"][1]["options"],
        json!(["NEW", "PROPOSAL", "CUSTOMER"])
    );
    assert_eq!(v["fields"][1]["required"], true);
    assert_eq!(
        fake.requests().last().unwrap().path,
        "/rest/metadata/objects/obj-opps"
    );

    let e = error_text(
        &s.describe_object(params(json!({"object": "unicorns"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("not_found: no object named unicorns"), "{e}");
}
