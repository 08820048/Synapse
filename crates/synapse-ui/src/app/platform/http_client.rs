use std::sync::Arc;

use futures::AsyncReadExt as _;
use gpui::http_client::{
    AsyncBody, HttpClient, Request, Response, Url,
    http::{HeaderValue, header::USER_AGENT},
};

pub(crate) struct SynapseHttpClient {
    client: reqwest::Client,
    runtime: Arc<tokio::runtime::Runtime>,
    user_agent: HeaderValue,
}

async fn execute_request(
    client: reqwest::Client,
    request: reqwest::Request,
) -> gpui::http_client::Result<Response<AsyncBody>> {
    let response = client.execute(request).await?;
    let status = response.status();
    let version = response.version();
    let headers = response.headers().clone();
    let body = response.bytes().await?.to_vec();

    let mut response = Response::builder().status(status).version(version);
    *response
        .headers_mut()
        .expect("HTTP response builder must expose headers") = headers;
    Ok(response.body(AsyncBody::from(body))?)
}

impl SynapseHttpClient {
    pub(crate) fn new() -> gpui::http_client::Result<Arc<Self>> {
        let user_agent = HeaderValue::from_static(concat!("Synapse/", env!("CARGO_PKG_VERSION")));
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("synapse-http")
                .enable_all()
                .build()?,
        );
        let client = {
            let _runtime_guard = runtime.enter();
            reqwest::Client::builder()
                .user_agent(user_agent.clone())
                .redirect_policy(reqwest::redirect::Policy::limited(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()?
        };
        Ok(Arc::new(Self {
            client,
            runtime,
            user_agent,
        }))
    }
}

impl HttpClient for SynapseHttpClient {
    fn type_name(&self) -> &'static str {
        "SynapseHttpClient"
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        request: Request<AsyncBody>,
    ) -> futures::future::BoxFuture<'static, gpui::http_client::Result<Response<AsyncBody>>> {
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        let user_agent = self.user_agent.clone();
        Box::pin(async move {
            let (mut parts, mut body) = request.into_parts();
            if !parts.headers.contains_key(USER_AGENT) {
                parts.headers.insert(USER_AGENT, user_agent);
            }

            let mut request_body = Vec::new();
            body.read_to_end(&mut request_body).await?;
            let request = Request::from_parts(parts, request_body);
            let request = reqwest::Request::try_from(request)?;
            runtime.spawn(execute_request(client, request)).await?
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        thread,
    };

    use futures::{AsyncReadExt as _, executor::block_on};
    use gpui::http_client::HttpClient as _;

    use super::SynapseHttpClient;

    #[test]
    fn real_http_client_has_a_synapse_user_agent() {
        let client = SynapseHttpClient::new().expect("HTTP client");
        assert_eq!(client.type_name(), "SynapseHttpClient");
        assert_eq!(
            client.user_agent().and_then(|value| value.to_str().ok()),
            Some(concat!("Synapse/", env!("CARGO_PKG_VERSION")))
        );
    }

    #[test]
    fn requests_run_on_the_owned_tokio_reactor_from_a_non_tokio_executor() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP listener");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("HTTP client connection");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("HTTP request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .expect("HTTP response");
        });

        let client = SynapseHttpClient::new().expect("HTTP client");
        let mut response =
            block_on(client.get(&format!("http://{address}/image.jpg"), ().into(), true))
                .expect("request outside Tokio context");
        let mut body = String::new();
        block_on(response.body_mut().read_to_string(&mut body)).expect("response body");

        assert!(response.status().is_success());
        assert_eq!(body, "OK");
        server.join().expect("local HTTP server");
    }
}
