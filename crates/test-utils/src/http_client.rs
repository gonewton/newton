#[cfg(feature = "http")]
use axum::Router;
#[cfg(feature = "http")]
use reqwest::Client;

#[cfg(feature = "http")]
pub struct TestServer {
    pub client: Client,
    pub base_url: String,
    _server: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "http")]
impl TestServer {
    pub async fn new(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test server");
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Test server failed");
        });

        let client = Client::new();

        TestServer {
            client,
            base_url,
            _server: server,
        }
    }
}
