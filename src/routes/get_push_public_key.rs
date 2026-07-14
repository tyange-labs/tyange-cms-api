use poem::{handler, web::Json, Error};

use crate::{
    models::{CustomResponse, PublicPushKeyResponse},
    web_push::push_public_key,
};

#[handler]
pub async fn get_push_public_key() -> Result<Json<CustomResponse<PublicPushKeyResponse>>, Error> {
    let public_key =
        push_public_key().map_err(|err| Error::from_string(err.message, err.status))?;

    Ok(Json(CustomResponse {
        status: true,
        data: Some(PublicPushKeyResponse { public_key }),
        message: None,
    }))
}
