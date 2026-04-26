//! Embedded OTLP collector used by `tests/e2e/test_otel_smoke.sh`.
//!
//! Listens on two ephemeral 127.0.0.1 ports — one for OTLP/HTTP-protobuf
//! (axum) and one for OTLP/gRPC (tonic) — and exposes a `__received` JSON
//! endpoint so the e2e script can poll how many export batches each transport
//! has accepted. We deliberately do not decode the payloads: the goal is to
//! detect "exporter silently disabled" bugs via the count, not to validate
//! protobuf semantics.

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::Router;
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
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

#[derive(Clone, Default)]
struct Counters {
    logs: Arc<AtomicU64>,
    spans: Arc<AtomicU64>,
}

async fn ingest_logs(State(c): State<Counters>) -> impl IntoResponse {
    c.logs.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        Vec::<u8>::new(),
    )
}

async fn ingest_traces(State(c): State<Counters>) -> impl IntoResponse {
    c.spans.fetch_add(1, Ordering::Relaxed);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        Vec::<u8>::new(),
    )
}

async fn received(State(c): State<Counters>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "logs": c.logs.load(Ordering::Relaxed),
        "spans": c.spans.load(Ordering::Relaxed),
    }))
}

#[derive(Clone)]
struct LogsSvc(Counters);

#[tonic::async_trait]
impl LogsService for LogsSvc {
    async fn export(
        &self,
        _req: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        self.0.logs.fetch_add(1, Ordering::Relaxed);
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

#[derive(Clone)]
struct TraceSvc(Counters);

#[tonic::async_trait]
impl TraceService for TraceSvc {
    async fn export(
        &self,
        _req: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        self.0.spans.fetch_add(1, Ordering::Relaxed);
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
