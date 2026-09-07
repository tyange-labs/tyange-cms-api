use std::env;

use dotenv::dotenv;
use tyange_cms_api::auth::jwt::Claims;

#[test]
fn test_validate_token() {
    dotenv().ok();

    let secret = env::var("JWT_ACCESS_SECRET").unwrap();
    let claims = Claims::new("test", "admin", 15);
    let access_token = claims.to_token(&secret.as_bytes()).unwrap();
    let result = Claims::validate_token(&access_token, &secret.as_bytes()).unwrap();
    assert!(result);
}
