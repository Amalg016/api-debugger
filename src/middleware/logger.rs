use actix_web::body::{BoxBody, EitherBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::web::{self, Bytes};
use actix_web::Error;
use sqlx::SqlitePool;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use tracing::{info, warn};

/// Logging middleware that intercepts every request/response and persists
/// them to the SQLite database.
///
/// ## How it handles the body-consumption problem
///
/// Actix streams the request body; once read it's gone.
/// For the *response* body we:
///   1. Let the inner handler run normally.
///   2. Split the `ServiceResponse` into `(HttpRequest, HttpResponse<B>)`.
///   3. Further split the `HttpResponse` into `(head, body)`.
///   4. Buffer the body bytes, log them, then reattach via `head.set_body()`.
pub struct RequestLogger;

impl<S, B> Transform<S, ServiceRequest> for RequestLogger
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<BoxBody>>;
    type Error = Error;
    type Transform = RequestLoggerMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestLoggerMiddleware { service }))
    }
}

pub struct RequestLoggerMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for RequestLoggerMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<BoxBody>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &self,
        ctx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let method = req.method().to_string();
        let path = req.uri().path().to_string();

        // Skip logging for UI asset requests to keep the DB clean
        if path.starts_with("/ui") || path == "/" {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                let (http_req, http_res) = res.into_parts();
                let (res_head, res_body) = http_res.into_parts();
                let body_bytes = actix_web::body::to_bytes(res_body)
                    .await
                    .unwrap_or_else(|_| Bytes::new());
                let rebuilt = res_head.set_body(BoxBody::new(body_bytes));
                Ok(ServiceResponse::new(http_req, rebuilt).map_into_right_body())
            });
        }

        let headers: serde_json::Value = req
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    serde_json::Value::String(v.to_str().unwrap_or("").to_owned()),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
        let headers_str = serde_json::to_string(&headers).unwrap_or_default();

        let pool = req
            .app_data::<web::Data<SqlitePool>>()
            .expect("SqlitePool not found in app_data")
            .clone();

        let fut = self.service.call(req);

        Box::pin(async move {
            let request_body: Option<String> = None;
            let res: ServiceResponse<B> = fut.await?;

            let status = res.status().as_u16() as i64;
            let (http_req, http_res) = res.into_parts();
            let (res_head, res_body) = http_res.into_parts();

            let body_bytes = actix_web::body::to_bytes(res_body)
                .await
                .unwrap_or_else(|_| Bytes::new());
            let response_body_str = String::from_utf8_lossy(&body_bytes).to_string();

            let db_result = insert_log(
                &pool,
                &method,
                &path,
                &headers_str,
                request_body.as_deref(),
                status,
                &response_body_str,
            )
            .await;

            if let Err(e) = db_result {
                warn!(error = %e, "Failed to log request to database");
            }

            let rebuilt = res_head.set_body(BoxBody::new(body_bytes));
            Ok(ServiceResponse::new(http_req, rebuilt).map_into_right_body())
        })
    }
}

async fn insert_log(
    pool: &SqlitePool,
    method: &str,
    path: &str,
    headers: &str,
    body: Option<&str>,
    status: i64,
    response_body: &str,
) -> Result<(), sqlx::Error> {
    let req_id = sqlx::query(
        "INSERT INTO requests (method, path, headers, body) VALUES (?, ?, ?, ?)",
    )
    .bind(method)
    .bind(path)
    .bind(headers)
    .bind(body)
    .execute(pool)
    .await?
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO responses (request_id, status, body) VALUES (?, ?, ?)",
    )
    .bind(req_id)
    .bind(status)
    .bind(response_body)
    .execute(pool)
    .await?;

    info!(
        method = method,
        path = path,
        status = status,
        request_id = req_id,
        "Request logged"
    );
    Ok(())
}
