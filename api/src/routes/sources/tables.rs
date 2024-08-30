use actix_web::{
    get,
    http::{header::ContentType, StatusCode},
    web::{Data, Json, Path},
    HttpRequest, HttpResponse, Responder, ResponseError,
};
use sqlx::PgPool;
use thiserror::Error;

use crate::{
    db::{self, sources::SourceConfig},
    routes::ErrorMessage,
};

#[derive(Debug, Error)]
enum TableError {
    #[error("database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("source with id {0} not found")]
    SourceNotFound(i64),

    #[error("tenant id missing in request")]
    TenantIdMissing,

    #[error("tenant id ill formed in request")]
    TenantIdIllFormed,

    #[error("invalid source config")]
    InvalidConfig(#[from] serde_json::Error),
}

impl TableError {
    fn to_message(&self) -> String {
        match self {
            // Do not expose internal database details in error messages
            TableError::DatabaseError(_) => "internal server error".to_string(),
            // Every other message is ok, as they do not divulge sensitive information
            e => e.to_string(),
        }
    }
}

impl ResponseError for TableError {
    fn status_code(&self) -> StatusCode {
        match self {
            TableError::DatabaseError(_) | TableError::InvalidConfig(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            TableError::SourceNotFound(_) => StatusCode::NOT_FOUND,
            TableError::TenantIdMissing | TableError::TenantIdIllFormed => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let error_message = ErrorMessage {
            error: self.to_message(),
        };
        let body =
            serde_json::to_string(&error_message).expect("failed to serialize error message");
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(body)
    }
}

// TODO: read tenant_id from a jwt
fn extract_tenant_id(req: &HttpRequest) -> Result<i64, TableError> {
    let headers = req.headers();
    let tenant_id = headers
        .get("tenant_id")
        .ok_or(TableError::TenantIdMissing)?;
    let tenant_id = tenant_id
        .to_str()
        .map_err(|_| TableError::TenantIdIllFormed)?;
    let tenant_id: i64 = tenant_id
        .parse()
        .map_err(|_| TableError::TenantIdIllFormed)?;
    Ok(tenant_id)
}

#[get("/sources/{source_id}/table_names")]
pub async fn read_table_names(
    req: HttpRequest,
    pool: Data<PgPool>,
    source_id: Path<i64>,
) -> Result<impl Responder, TableError> {
    let tenant_id = extract_tenant_id(&req)?;
    let source_id = source_id.into_inner();

    let config = db::sources::read_source(&pool, tenant_id, source_id)
        .await?
        .map(|s| {
            let config: SourceConfig = serde_json::from_value(s.config)?;
            Ok::<SourceConfig, serde_json::Error>(config)
        })
        .transpose()?
        .ok_or(TableError::SourceNotFound(source_id))?;

    let options = config.connect_options();
    let tables = db::tables::get_tables(&options).await?;

    Ok(Json(tables))
}
