// Suppress warnings for Dropshot's macro-generated phantom types
// Figure out why RequestContext does this
#![allow(dead_code)]

use dropshot::{
    ApiDescription, Body, HttpError, HttpResponseOk, Query, RequestContext,
    endpoint,
};
use http::{Response, StatusCode};
use oxmon_common::{HostStatus, HostTimeline};
use oxmon_core::Monitor;
use slog::{Drain, Logger, o};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::web::render_dashboard;

pub struct ServerContext {
    pub monitor: Arc<Monitor>,
}

/// Query parameters for the timelines endpoint
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TimelineQuery {
    /// Number of buckets to display (default: 20)
    #[serde(default = "default_num_buckets")]
    num_buckets: usize,
}

fn default_num_buckets() -> usize {
    20
}

#[endpoint {
    method = GET,
    path = "/api/hosts",
}]
async fn get_hosts(
    ctx: RequestContext<ServerContext>,
) -> Result<HttpResponseOk<Vec<HostStatus>>, HttpError> {
    let status = ctx.context().monitor.get_status().await;
    Ok(HttpResponseOk(status))
}

#[endpoint {
    method = GET,
    path = "/api/timelines",
}]
async fn get_timelines(
    ctx: RequestContext<ServerContext>,
    query: Query<TimelineQuery>,
) -> Result<HttpResponseOk<Vec<HostTimeline>>, HttpError> {
    let params = query.into_inner();
    let timelines = ctx
        .context()
        .monitor
        .get_timelines(params.num_buckets)
        .await
        .map_err(|e| {
            HttpError::for_internal_error(format!(
                "failed to get timelines: {}",
                e
            ))
        })?;
    Ok(HttpResponseOk(timelines))
}

#[allow(unused)]
#[endpoint {
    method = GET,
    path = "/",
}]
async fn get_dashboard(
    ctx: RequestContext<ServerContext>,
    query: Query<TimelineQuery>,
) -> Result<Response<Body>, HttpError> {
    let params = query.into_inner();
    let timelines = ctx
        .context()
        .monitor
        .get_timelines(params.num_buckets)
        .await
        .map_err(|e| {
            HttpError::for_internal_error(format!(
                "failed to get timelines: {}",
                e
            ))
        })?;
    let html = render_dashboard(&timelines);

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(html.into())
        .map_err(|e| {
            HttpError::for_internal_error(format!(
                "failed to build response: {}",
                e
            ))
        })
}

pub async fn start_server(
    addr: SocketAddr,
    monitor: Arc<Monitor>,
) -> anyhow::Result<()> {
    let mut api = ApiDescription::new();
    api.register(get_hosts)?;
    api.register(get_timelines)?;
    api.register(get_dashboard)?;

    let context = ServerContext { monitor };

    // Create logger
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = Logger::root(drain, o!());

    let server = dropshot::HttpServerStarter::new(
        &dropshot::ConfigDropshot {
            bind_address: addr,
            ..Default::default()
        },
        api,
        context,
        &log,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?
    .start();

    server.await.map_err(|e| anyhow::anyhow!("{}", e))
}
