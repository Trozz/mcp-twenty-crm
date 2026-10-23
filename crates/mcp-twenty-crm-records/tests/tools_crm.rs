mod support;
use serde_json::json;
use support::fake_twenty::{FakeState, FakeTwenty, Injected, uuid};
use support::tools::{auth, error_text, json, params, server};

async fn fake() -> FakeTwenty {
    FakeTwenty::start(FakeState::workspace()).await
}

#[tokio::test]
async fn create_person_builds_the_composite_fields() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .create_person(
                params(json!({
                    "first_name": " Cara ",
                    "last_name": "Diaz",
                    "email": "cara@acme.example",
                    "phone": "7700 900123",
                    "phone_calling_code": "+44",
                    "job_title": "Buyer",
                    "company_id": uuid(1),
                    "linkedin_url": "https://linkedin.com/in/cara",
                    "extra": {"tier": "A", "jobTitle": "ignored"}
                })),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(v["id"].is_string());
    let body = fake.requests()[0].body.clone().unwrap();
    assert_eq!(
        body,
        json!({
            "name": {"firstName": "Cara", "lastName": "Diaz"},
            "emails": {"primaryEmail": "cara@acme.example"},
            "phones": {"primaryPhoneNumber": "7700 900123", "primaryPhoneCallingCode": "+44"},
            "jobTitle": "Buyer",
            "companyId": uuid(1),
            "linkedinLink": {"primaryLinkUrl": "https://linkedin.com/in/cara"},
            "tier": "A"
        })
    );
    assert_eq!(fake.requests()[0].path, "/rest/people");
}

#[tokio::test]
async fn create_company_builds_address_and_links() {
    let fake = fake().await;
    json(
        &server(&fake)
            .create_company(
                params(json!({
                    "name": "Initech",
                    "domain": "https://initech.example",
                    "employees": 40,
                    "city": "Leeds",
                    "country": "United Kingdom"
                })),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        fake.requests()[0].body.clone().unwrap(),
        json!({
            "name": "Initech",
            "domainName": {"primaryLinkUrl": "https://initech.example"},
            "employees": 40,
            "address": {"addressCity": "Leeds", "addressCountry": "United Kingdom"}
        })
    );
}

#[tokio::test]
async fn create_opportunity_converts_the_amount_to_micros() {
    let fake = fake().await;
    let s = server(&fake);
    json(
        &s.create_opportunity(
            params(json!({"name": "Big deal", "amount": 12500.5, "stage": "proposal", "company_id": uuid(1)})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        fake.requests()[0].body.clone().unwrap(),
        json!({
            "name": "Big deal",
            "amount": {"amountMicros": 12_500_500_000_i64, "currencyCode": "GBP"},
            "stage": "PROPOSAL",
            "companyId": uuid(1)
        })
    );
    json(
        &s.create_opportunity(
            params(json!({"name": "Euro deal", "amount": 10, "currency_code": "eur"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        fake.requests()[1].body.clone().unwrap()["amount"]["currencyCode"],
        "EUR"
    );
    let e = error_text(
        &s.create_opportunity(
            params(json!({"name": "x", "amount": 1, "currency_code": "pounds"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.starts_with("validation: currency_code"), "{e}");
}

#[tokio::test]
async fn create_note_links_every_target() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .create_note(
                params(json!({
                    "title": "Call with Ann",
                    "body": "## Agreed\n- send quote",
                    "person_ids": [uuid(11)],
                    "company_ids": [uuid(1)],
                    "opportunity_ids": [uuid(21)]
                })),
                auth(),
            )
            .await
            .unwrap(),
    );
    let note_id = v["record"]["id"].as_str().unwrap().to_string();
    assert_eq!(v["linked"].as_array().unwrap().len(), 3);
    assert!(v.get("link_errors").is_none());
    let log = fake.requests();
    assert_eq!(
        log[0].body.clone().unwrap(),
        json!({"title": "Call with Ann", "bodyV2": {"markdown": "## Agreed\n- send quote"}})
    );
    assert_eq!(log[1].path, "/rest/noteTargets");
    assert_eq!(
        log[1].body.clone().unwrap(),
        json!({"noteId": note_id, "targetPersonId": uuid(11)})
    );
    assert_eq!(
        log[2].body.clone().unwrap(),
        json!({"noteId": note_id, "targetCompanyId": uuid(1)})
    );
    assert_eq!(
        log[3].body.clone().unwrap(),
        json!({"noteId": note_id, "targetOpportunityId": uuid(21)})
    );
}

#[tokio::test]
async fn a_failed_link_is_reported_without_failing_the_note() {
    let fake = fake().await;
    let s = server(&fake);
    // The note is created, then the first link fails.
    fake.state().forbidden.push("noteTargets".into());
    let v = json(
        &s.create_note(
            params(json!({"title": "x", "person_ids": [uuid(11)]})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(v["record"]["id"].is_string());
    assert_eq!(v["linked"], json!([]));
    assert!(
        v["link_errors"][0]["error"]
            .as_str()
            .unwrap()
            .starts_with("permission: ")
    );
    assert!(
        v["note"]
            .as_str()
            .unwrap()
            .contains("do not create it again")
    );
}

#[tokio::test]
async fn create_task_checks_status_and_links_targets() {
    let fake = fake().await;
    let s = server(&fake);
    let e = error_text(
        &s.create_task(params(json!({"title": "x", "status": "maybe"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("validation: status"), "{e}");
    assert!(fake.requests().is_empty());

    let v = json(
        &s.create_task(
            params(json!({
                "title": "Send quote",
                "status": "in progress",
                "due_at": "2026-03-10T09:00:00Z",
                "assignee_id": uuid(51),
                "company_ids": [uuid(1)]
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    let task_id = v["record"]["id"].as_str().unwrap().to_string();
    let log = fake.requests();
    assert_eq!(
        log[0].body.clone().unwrap(),
        json!({"title": "Send quote", "status": "IN_PROGRESS", "dueAt": "2026-03-10T09:00:00Z", "assigneeId": uuid(51)})
    );
    assert_eq!(log[1].path, "/rest/taskTargets");
    assert_eq!(
        log[1].body.clone().unwrap(),
        json!({"taskId": task_id, "targetCompanyId": uuid(1)})
    );
}

#[tokio::test]
async fn record_activity_lists_linked_notes_and_tasks() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_record_activity(
                params(json!({"object": "companies", "id": uuid(1)})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(v["notes"]["count"], 1);
    assert_eq!(v["notes"]["items"][0]["title"], "Kick-off call");
    assert_eq!(
        v["notes"]["items"][0]["bodyV2"],
        json!({"markdown": "Went well"})
    );
    assert_eq!(v["tasks"]["count"], 0);
    let log = fake.requests();
    assert_eq!(log[0].path, "/rest/noteTargets");
    assert_eq!(
        log[0].param("filter"),
        Some(format!("targetCompanyId[eq]:{}", uuid(1)).as_str())
    );
    assert_eq!(log[0].param("depth"), Some("1"));
    assert_eq!(log[1].path, "/rest/taskTargets");
}

#[tokio::test]
async fn workspace_members_are_listed() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_workspace_members(params(json!({})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["items"][0]["userEmail"], "sam@example.com");
    assert_eq!(fake.requests()[0].path, "/rest/workspaceMembers");
}

#[tokio::test]
async fn a_rate_limit_on_create_is_not_retried() {
    let fake = fake().await;
    fake.inject(Injected::json(429, json!({})));
    let e = error_text(
        &server(&fake)
            .create_person(params(json!({"first_name": "X"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("rate_limited: "), "{e}");
    assert_eq!(fake.requests().len(), 1);
}
