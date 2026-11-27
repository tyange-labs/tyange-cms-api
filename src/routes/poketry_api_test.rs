use poem::{http::StatusCode, test::TestClient};
use poem_openapi::OpenApiService;
use serde_json::{json, Value};

use crate::poketry_api::PoketryApi;

async fn analyze(names: Value) -> (StatusCode, String) {
    post(json!({ "names": names })).await
}

async fn post(body: Value) -> (StatusCode, String) {
    let cli = TestClient::new(OpenApiService::new(PoketryApi, "test", "1.0"));
    let resp = cli.post("/poketry/analyze").body_json(&body).send().await;
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
    assert_eq!(slots[0]["covers"], json!(["전기", "바위"]));
    assert_eq!(slots[0]["spot_increase_if_removed"], json!(2));
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

fn data(body: &str) -> Value {
    serde_json::from_str::<Value>(body).unwrap()["data"].clone()
}

#[tokio::test]
async fn ids_give_the_same_result_as_names() {
    let (_, by_names) = analyze(json!(["리자몽", "가디안", "루카리오"])).await;
    let ids: Vec<Value> = data(&by_names)["team"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect();

    let (status, by_ids) = post(json!({ "ids": ids })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data(&by_ids), data(&by_names));
}

#[tokio::test]
async fn unknown_id_is_rejected() {
    let (status, body) = post(json!({ "ids": [6, 999999] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("999999"));
}

#[tokio::test]
async fn names_and_ids_must_not_be_sent_together_or_both_missing() {
    let (status, _) = post(json!({ "names": ["리자몽"], "ids": [6] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_same_pokemon_twice_is_rejected() {
    let (status, body) = analyze(json!(["리자몽", "가디안", "리자몽"])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("'리자몽' 이(가) 두 번 들어 있습니다."));

    let (status, _) = post(json!({ "ids": [6, 6] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn limit_caps_each_group() {
    let (status, body) = post(json!({ "names": ["리자몽"], "limit": 1 })).await;
    assert_eq!(status, StatusCode::OK);
    for slot in data(&body)["plan"]["slots"].as_array().unwrap() {
        assert_eq!(slot["picks"].as_array().unwrap().len(), 1);
    }

    let six = json!(["리자몽", "가디안", "루카리오", "갸라도스", "마기라스", "님피아"]);
    let (_, default_body) = analyze(six.clone()).await;
    let (_, small_body) = post(json!({ "names": six, "limit": 2 })).await;
    let default_fixes = data(&default_body)["fixes"].as_array().unwrap().clone();
    let small_fixes = data(&small_body)["fixes"].as_array().unwrap().clone();
    assert_eq!(default_fixes.len(), small_fixes.len());
    for (d, s) in default_fixes.iter().zip(&small_fixes) {
        let d = d["swaps"].as_array().unwrap();
        let s = s["swaps"].as_array().unwrap();
        assert!(d.len() <= 10);
        assert_eq!(s.len(), d.len().min(2));
        assert_eq!(s[..], d[..s.len()]);
    }
}

#[tokio::test]
async fn limit_out_of_range_is_rejected() {
    for limit in [0, 51] {
        let (status, _) = post(json!({ "names": ["리자몽"], "limit": limit })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "limit {limit}");
    }
}
