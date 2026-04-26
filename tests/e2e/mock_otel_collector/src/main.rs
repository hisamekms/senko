//! Embedded OTLP collector used by `tests/e2e/test_otel_smoke.sh`.
//!
//! Listens on two ephemeral 127.0.0.1 ports — one for OTLP/HTTP-protobuf
//! (axum) and one for OTLP/gRPC (tonic) — and exposes a `__received` JSON
//! endpoint so the e2e script can poll how many export batches each transport
//! has accepted. The collector decodes incoming payloads just enough to
//! capture the `service.name` Resource attribute per signal so tests can
//! assert relay/remote defaults and `OTEL_SERVICE_NAME` overrides.

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::{
    LogsService, LogsServiceServer,
};
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyValueKind;
use opentelemetry_proto::tonic::resource::v1::Resource as PbResource;
use prost::Message;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

#[derive(Clone, Default)]
struct Counters {
    logs: Arc<AtomicU64>,
    spans: Arc<AtomicU64>,
    /// Latest observed `service.name` Resource attribute, per signal. `None`
    /// until the first batch with a string-typed `service.name` arrives.
    logs_service_name: Arc<Mutex<Option<String>>>,
    traces_service_name: Arc<Mutex<Option<String>>>,
}

/// Find the string-typed `service.name` value on a protobuf `Resource`.
fn extract_service_name(resource: Option<&PbResource>) -> Option<String> {
    let res = resource?;
    for kv in &res.attributes {
        if kv.key == "service.name"
            && let Some(value) = &kv.value
            && let Some(AnyValueKind::StringValue(s)) = &value.value
        {
            return Some(s.clone());
        }
    }
    None
}

async fn record_logs_service_name(counters: &Counters, req: &ExportLogsServiceRequest) {
    for rl in &req.resource_logs {
        if let Some(name) = extract_service_name(rl.resource.as_ref()) {
            *counters.logs_service_name.lock().await = Some(name);
            return;
        }
    }
}

async fn record_traces_service_name(counters: &Counters, req: &ExportTraceServiceRequest) {
    for rs in &req.resource_spans {
        if let Some(name) = extract_service_name(rs.resource.as_ref()) {
            *counters.traces_service_name.lock().await = Some(name);
            return;
        }
    }
}

async fn ingest_logs(State(c): State<Counters>, body: Bytes) -> impl IntoResponse {
    c.logs.fetch_add(1, Ordering::Relaxed);
    if let Ok(req) = ExportLogsServiceRequest::decode(body.as_ref()) {
        record_logs_service_name(&c, &req).await;
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        Vec::<u8>::new(),
    )
}

async fn ingest_traces(State(c): State<Counters>, body: Bytes) -> impl IntoResponse {
    c.spans.fetch_add(1, Ordering::Relaxed);
    if let Ok(req) = ExportTraceServiceRequest::decode(body.as_ref()) {
        record_traces_service_name(&c, &req).await;
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        Vec::<u8>::new(),
    )
}

async fn received(State(c): State<Counters>) -> Json<serde_json::Value> {
    let logs_service_name = c.logs_service_name.lock().await.clone();
    let traces_service_name = c.traces_service_name.lock().await.clone();
    Json(serde_json::json!({
        "logs": c.logs.load(Ordering::Relaxed),
        "spans": c.spans.load(Ordering::Relaxed),
        "logs_service_name": logs_service_name,
        "traces_service_name": traces_service_name,
    }))
}

#[derive(Clone)]
struct LogsSvc(Counters);

#[tonic::async_trait]
impl LogsService for LogsSvc {
    async fn export(
        &self,
        req: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        self.0.logs.fetch_add(1, Ordering::Relaxed);
        record_logs_service_name(&self.0, req.get_ref()).await;
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

#[derive(Clone)]
struct TraceSvc(Counters);

#[tonic::async_trait]
impl TraceService for TraceSvc {
    async fn export(
        &self,
        req: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        self.0.spans.fetch_add(1, Ordering::Relaxed);
        record_traces_service_name(&self.0, req.get_ref()).await;
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let counters = Counters::default();

    let http_listener = TcpListener::bind("127.0.0.1:0").await?;
    let http_port = http_listener.local_addr()?.port();

    let grpc_listener = TcpListener::bind("127.0.0.1:0").await?;
    let grpc_port = grpc_listener.local_addr()?.port();

    // Announce both ports on stdout BEFORE starting the servers so the e2e
    // driver's `wait_for` doesn't race against bind. Flush so the test sees
    // them even if our stdout is line-buffered through a pipe.
    println!("listen-port-http={http_port}");
    println!("listen-port-grpc={grpc_port}");
    std::io::stdout().flush().ok();

    let app = Router::new()
        .route("/v1/logs", post(ingest_logs))
        .route("/v1/traces", post(ingest_traces))
        .route("/__received", get(received))
        .with_state(counters.clone());

    let http_task = tokio::spawn(async move { axum::serve(http_listener, app).await });

    let grpc_task = {
        let counters = counters.clone();
        tokio::spawn(async move {
            Server::builder()
                .add_service(LogsServiceServer::new(LogsSvc(counters.clone())))
                .add_service(TraceServiceServer::new(TraceSvc(counters)))
                .serve_with_incoming(TcpListenerStream::new(grpc_listener))
                .await
        })
    };

    tokio::select! {
        r = http_task => r??,
        r = grpc_task => r??,
    }

    Ok(())
}
