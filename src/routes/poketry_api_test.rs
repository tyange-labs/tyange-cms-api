use poem::{http::StatusCode, test::TestClient};
use poem_openapi::OpenApiService;
use serde_json::{json, Value};

use crate::poketry_api::PoketryApi;

async fn analyze(names: Value) -> (StatusCode, String) {
    let cli = TestClient::new(OpenApiService::new(PoketryApi, "test", "1.0"));
    let resp = cli
        .post("/poketry/analyze")
        .body_json(&json!({ "names": names }))
        .send()
        .await;
    let status = resp.0.status();
    let body = resp.0.into_body().into_string().await.unwrap();
    (status, body)
}

#[tokio::test]
async fn five_members_get_a_slot_plan() {
    let (status, body) = analyze(json!(["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"])).await;
    assert_eq!(status, StatusCode::OK);

    let data = &serde_json::from_str::<Value>(&body).unwrap()["data"];
    assert_eq!(data["team"].as_array().unwrap().len(), 5);
    assert_eq!(data["team"][0]["types"], json!(["불꽃", "비행"]));
    assert_eq!(data["weaknesses"].as_array().unwrap().len(), 18);

    let slots = data["plan"]["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 1);
    assert!(!slots[0]["picks"].as_array().unwrap().is_empty());
    assert_eq!(data["fixes"], json!([]));
}

#[tokio::test]
async fn six_members_get_fixes_per_weak_type() {
    let (status, body) = analyze(json!([
        "리자몽", "가디안", "루카리오", "갸라도스", "마기라스", "님피아"
    ]))
    .await;
    assert_eq!(status, StatusCode::OK);

    let data = &serde_json::from_str::<Value>(&body).unwrap()["data"];
    assert!(data.get("plan").map_or(true, Value::is_null));

    let weak_spots: Vec<&str> = data["weak_spots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    let fix_types: Vec<&str> = data["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["weak_type"].as_str().unwrap())
        .collect();
    assert!(!weak_spots.is_empty());
    assert_eq!(fix_types, weak_spots);
}

#[tokio::test]
async fn unknown_name_is_rejected() {
    let (status, body) = analyze(json!(["없는몬"])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("없는몬"));
}

#[tokio::test]
async fn empty_or_too_many_members_are_rejected() {
    let (status, _) = analyze(json!([])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = analyze(json!(vec!["리자몽"; 7])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
