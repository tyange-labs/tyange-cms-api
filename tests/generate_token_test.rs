use std::env;

use dotenv::dotenv;
use tyange_cms_api::auth::jwt::Claims;

#[test]
fn test_generate_token() {
    dotenv().ok();

    let secret = env::var("JWT_ACCESS_SECRET").unwrap();
    let claims = Claims::new("test", "admin", 15);
    let access_token = claims.to_token(&secret.as_bytes()).unwrap();
    println!("Generated token: {}", access_token);
}
