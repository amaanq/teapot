use std::{
   io::Read as _,
   sync::Arc,
   time::Duration,
};

use axum::http::{
   HeaderMap,
   Method,
   Uri,
   header,
};
use bytes::Bytes;
use flate2::read::GzDecoder;
use http_body_util::{
   BodyExt as _,
   Empty,
};
use hyper::{
   StatusCode,
   body as hyper_body,
};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{
   client::legacy::{
      Client,
      connect::HttpConnector,
   },
   rt::TokioExecutor,
};
use serde::de::DeserializeOwned;
use tokio::{
   io::{
      AsyncReadExt as _,
      AsyncWriteExt as _,
   },
   net::TcpStream,
};

use crate::error::{
   Error,
   Result,
};

type Connector = hyper_rustls::HttpsConnector<HttpConnector>;

/// Parsed proxy configuration.
#[derive(Clone)]
struct ProxyConfig {
   host: String,
   port: u16,
   auth: Option<String>,
}

/// Lightweight HTTP client wrapping hyper-util's connection-pooling client.
///
/// When a proxy is configured, HTTPS requests are tunneled via HTTP CONNECT.
#[derive(Clone)]
#[expect(
   clippy::module_name_repetitions,
   reason = "HttpClient is clearer than Client"
)]
pub struct HttpClient {
   inner:           Client<Connector, Empty<Bytes>>,
   default_headers: HeaderMap,
   proxy:           Option<ProxyConfig>,
   tls:             Arc<rustls::ClientConfig>,
}

/// Response wrapper providing convenience methods.
pub struct Response {
   status:  StatusCode,
   headers: HeaderMap,
   body:    hyper_body::Incoming,
}

impl HttpClient {
   pub fn new(proxy_url: &str, proxy_auth: &str) -> Self {
      let roots = rustls_native_certs::load_native_certs()
         .certs
         .into_iter()
         .fold(rustls::RootCertStore::empty(), |mut store, cert| {
            let _ = store.add(cert);
            store
         });

      let tls = Arc::new(
         rustls::ClientConfig::builder()
            .with_root_certificates(roots.clone())
            .with_no_client_auth(),
      );

      let connector = HttpsConnectorBuilder::new()
         .with_tls_config(
            rustls::ClientConfig::builder()
               .with_root_certificates(roots)
               .with_no_client_auth(),
         )
         .https_or_http()
         .enable_http1()
         .build();

      let inner = Client::builder(TokioExecutor::new())
         .pool_idle_timeout(Duration::from_secs(90))
         .build(connector);

      let proxy = if proxy_url.is_empty() {
         None
      } else {
         Some(parse_proxy(proxy_url, proxy_auth))
      };

      Self {
         inner,
         default_headers: HeaderMap::new(),
         proxy,
         tls,
      }
   }

   /// Create a client with default headers applied to every request.
   pub fn with_default_headers(mut self, headers: HeaderMap) -> Self {
      self.default_headers = headers;
      self
   }

   /// Send a request with a given method and optional extra headers.
   async fn send(&self, method: Method, uri: &str, extra_headers: &HeaderMap) -> Result<Response> {
      if let Some(proxy) = &self.proxy {
         return self.send_via_proxy(proxy, method, uri, extra_headers).await;
      }
      self.send_direct(method, uri, extra_headers).await
   }

   /// Direct request through hyper's connection pool.
   async fn send_direct(
      &self,
      method: Method,
      uri: &str,
      extra_headers: &HeaderMap,
   ) -> Result<Response> {
      let parsed: Uri = uri
         .parse()
         .map_err(|err| Error::Internal(format!("invalid URI: {err}")))?;

      let mut builder = hyper::Request::builder().method(method).uri(parsed);
      for (key, value) in &self.default_headers {
         builder = builder.header(key, value);
      }
      for (key, value) in extra_headers {
         builder = builder.header(key, value);
      }

      let request = builder
         .body(Empty::<Bytes>::new())
         .map_err(|err| Error::Internal(format!("build request: {err}")))?;

      let resp = self
         .inner
         .request(request)
         .await
         .map_err(|err| Error::Internal(format!("HTTP request failed: {err}")))?;

      let (parts, body) = resp.into_parts();
      Ok(Response {
         status: parts.status,
         headers: parts.headers,
         body,
      })
   }

   /// Send request through HTTP CONNECT proxy tunnel.
   async fn send_via_proxy(
      &self,
      proxy: &ProxyConfig,
      method: Method,
      uri: &str,
      extra_headers: &HeaderMap,
   ) -> Result<Response> {
      let parsed: Uri = uri
         .parse()
         .map_err(|err| Error::Internal(format!("invalid URI: {err}")))?;

      let target_host = parsed
         .host()
         .ok_or_else(|| Error::Internal("no host in URI".into()))?;
      let target_port = parsed.port_u16().unwrap_or(if parsed.scheme_str() == Some("https") {
         443
      } else {
         80
      });
      let is_https = parsed.scheme_str() == Some("https");

      // TCP connect to proxy
      let mut stream = TcpStream::connect((&*proxy.host, proxy.port))
         .await
         .map_err(|err| Error::Internal(format!("proxy connect: {err}")))?;

      if is_https {
         // CONNECT handshake
         let mut connect_req = format!("CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n");
         if let Some(auth) = &proxy.auth {
            connect_req.push_str(&format!("Proxy-Authorization: Basic {auth}\r\n"));
         }
         connect_req.push_str("\r\n");

         stream
            .write_all(connect_req.as_bytes())
            .await
            .map_err(|err| Error::Internal(format!("proxy CONNECT write: {err}")))?;

         // Read CONNECT response — look for end of HTTP headers
         let mut buf = vec![0u8; 4096];
         let mut filled = 0;
         loop {
            let n = stream
               .read(&mut buf[filled..])
               .await
               .map_err(|err| Error::Internal(format!("proxy CONNECT read: {err}")))?;
            if n == 0 {
               return Err(Error::Internal("proxy closed during CONNECT".into()));
            }
            filled += n;
            if filled >= 4
               && buf[..filled]
                  .windows(4)
                  .any(|w| w == b"\r\n\r\n")
            {
               break;
            }
            if filled >= buf.len() {
               return Err(Error::Internal("proxy CONNECT response too large".into()));
            }
         }

         let response_line = std::str::from_utf8(&buf[..filled])
            .map_err(|_| Error::Internal("proxy CONNECT: invalid UTF-8".into()))?;
         if !response_line.starts_with("HTTP/1.1 200") && !response_line.starts_with("HTTP/1.0 200")
         {
            let first_line = response_line.lines().next().unwrap_or("(empty)");
            return Err(Error::Internal(format!("proxy CONNECT rejected: {first_line}")));
         }

         // TLS handshake over the tunnel
         let server_name = rustls::pki_types::ServerName::try_from(target_host.to_owned())
            .map_err(|err| Error::Internal(format!("invalid server name: {err}")))?;

         let tls_connector = tokio_rustls::TlsConnector::from(Arc::clone(&self.tls));
         let tls_stream = tls_connector
            .connect(server_name, stream)
            .await
            .map_err(|err| Error::Internal(format!("proxy TLS handshake: {err}")))?;

         // HTTP/1.1 over the TLS tunnel
         let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(
            hyper_util::rt::TokioIo::new(tls_stream),
         )
         .await
         .map_err(|err| Error::Internal(format!("proxy HTTP handshake: {err}")))?;

         tokio::spawn(async move {
            if let Err(err) = conn.await {
               tracing::debug!("proxy connection closed: {err}");
            }
         });

         let path_and_query = parsed
            .path_and_query()
            .map_or("/", hyper::http::uri::PathAndQuery::as_str);
         let mut builder = hyper::Request::builder()
            .method(method)
            .uri(path_and_query)
            .header(header::HOST, target_host);

         for (key, value) in &self.default_headers {
            builder = builder.header(key, value);
         }
         for (key, value) in extra_headers {
            builder = builder.header(key, value);
         }

         let request = builder
            .body(Empty::<Bytes>::new())
            .map_err(|err| Error::Internal(format!("build proxied request: {err}")))?;

         let resp = sender
            .send_request(request)
            .await
            .map_err(|err| Error::Internal(format!("proxied request failed: {err}")))?;

         let (parts, body) = resp.into_parts();
         Ok(Response {
            status: parts.status,
            headers: parts.headers,
            body,
         })
      } else {
         // Plain HTTP through proxy: send request with absolute URI
         let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(
            hyper_util::rt::TokioIo::new(stream),
         )
         .await
         .map_err(|err| Error::Internal(format!("proxy HTTP handshake: {err}")))?;

         tokio::spawn(async move {
            if let Err(err) = conn.await {
               tracing::debug!("proxy connection closed: {err}");
            }
         });

         let mut builder = hyper::Request::builder()
            .method(method)
            .uri(uri) // absolute URI for HTTP proxy
            .header(header::HOST, target_host);

         if let Some(auth) = &proxy.auth {
            builder = builder.header("Proxy-Authorization", format!("Basic {auth}"));
         }
         for (key, value) in &self.default_headers {
            builder = builder.header(key, value);
         }
         for (key, value) in extra_headers {
            builder = builder.header(key, value);
         }

         let request = builder
            .body(Empty::<Bytes>::new())
            .map_err(|err| Error::Internal(format!("build proxied request: {err}")))?;

         let resp = sender
            .send_request(request)
            .await
            .map_err(|err| Error::Internal(format!("proxied request failed: {err}")))?;

         let (parts, body) = resp.into_parts();
         Ok(Response {
            status: parts.status,
            headers: parts.headers,
            body,
         })
      }
   }

   /// Send a GET request.
   pub async fn get(&self, uri: &str) -> Result<Response> {
      self.send(Method::GET, uri, &HeaderMap::new()).await
   }

   /// Send a GET request with additional headers.
   pub async fn get_with_headers(&self, uri: &str, extra_headers: &HeaderMap) -> Result<Response> {
      self.send(Method::GET, uri, extra_headers).await
   }

   /// Send a HEAD request.
   pub async fn head(&self, uri: &str) -> Result<Response> {
      self.send(Method::HEAD, uri, &HeaderMap::new()).await
   }
}

/// Parse proxy URL (e.g. `http://host:port`) and optional `user:pass` auth.
fn parse_proxy(url: &str, auth: &str) -> ProxyConfig {
   let stripped = url
      .strip_prefix("https://")
      .or_else(|| url.strip_prefix("http://"))
      .unwrap_or(url);
   let (host, port) = if let Some((h, p)) = stripped.rsplit_once(':') {
      (h.to_owned(), p.parse().unwrap_or(8080))
   } else {
      (stripped.to_owned(), 8080)
   };
   let auth = if auth.is_empty() {
      None
   } else {
      Some(data_encoding::BASE64.encode(auth.as_bytes()))
   };
   ProxyConfig { host, port, auth }
}

impl Response {
   pub const fn status(&self) -> StatusCode {
      self.status
   }

   pub const fn headers(&self) -> &HeaderMap {
      &self.headers
   }

   /// Collect the response body as bytes, decompressing gzip if needed.
   pub async fn bytes(self) -> Result<Bytes> {
      let is_gzip = self
         .headers
         .get(header::CONTENT_ENCODING)
         .and_then(|val| val.to_str().ok())
         .is_some_and(|val| val.contains("gzip"));

      let collected = self
         .body
         .collect()
         .await
         .map_err(|err| Error::Internal(format!("read body: {err}")))?
         .to_bytes();

      if is_gzip {
         let mut gz = GzDecoder::new(&*collected);
         let mut decoded = Vec::new();
         gz.read_to_end(&mut decoded)
            .map_err(|err| Error::Internal(format!("gzip decode: {err}")))?;
         Ok(Bytes::from(decoded))
      } else {
         Ok(collected)
      }
   }

   /// Collect the response body as a UTF-8 string.
   pub async fn text(self) -> Result<String> {
      let data = self.bytes().await?;
      String::from_utf8(data.to_vec())
         .map_err(|err| Error::Internal(format!("invalid UTF-8: {err}")))
   }

   /// Deserialize the response body as JSON.
   pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
      let data = self.bytes().await?;
      serde_json::from_slice(&data).map_err(Into::into)
   }
}
