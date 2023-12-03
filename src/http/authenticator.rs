use std::borrow::Cow;
use std::error;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

use async_session::{SessionStore, Session};
use async_trait::async_trait;

use cookie::Cookie;

use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::http::uri::{Authority, Scheme};
use hyper::{header, Request, Response, StatusCode, Uri};

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::http::uri::InvalidUri;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, HttpRequest, HttpResponse,
    IssuerUrl, Nonce, RedirectUrl, TokenResponse
};

use serde_derive::{Deserialize, Serialize};

use crate::error::Error;
use crate::http::HttpService;

use super::{Client, HttpContext};

pub struct AuthenticatorService {
    pub service: Arc<dyn HttpService + Send + Sync>,
    pub http_client: Client,
    pub client: CoreClient,
    pub session_cookie: String
}

#[derive(Deserialize)]
pub struct Token {}

#[derive(Debug)]
enum ClientError {
    Uri(InvalidUri),
    Other(&'static str)
}

impl error::Error for ClientError {}

impl Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Upstream server error")
    }
}

fn get_client<'a>(client: &'a Client) -> impl Fn(HttpRequest) -> Pin<Box<dyn Future<Output = Result<HttpResponse, ClientError>> + 'a + Send>> {
    move |req: HttpRequest| Box::pin(async move {
        let uri = Uri::try_from(req.url.as_str()).map_err(|e| ClientError::Uri(e))?;

        let mut request = Request::builder()
            .method(req.method).uri(uri.clone())
            .header(header::HOST, uri.host().ok_or(ClientError::Other("No hostname"))?);

        for (key, value) in req.headers {
            request = request.header(key.ok_or(ClientError::Other("Invalid header"))?, value);
        }

        let request = request
            .body(BoxBody::new(
                Full::new(Bytes::from(req.body)).map_err(|e| -> Error { Box::new(e) }),
            )).map_err(|_| ClientError::Other("Can't create response"))?;

        let mut conn = client.get_connection(&uri).await.map_err(|_| ClientError::Other("No connection"))?;
        let resp = conn.send_request(request).await.map_err(|_| ClientError::Other("Can't send request"))?;

        Ok(HttpResponse {
            status_code: resp.status(),
            headers: resp.headers().to_owned(),
            body: resp.collect().await.map_err(|_| ClientError::Other("Can't receive body"))?.to_bytes().to_vec(),
        })
    })
}

#[derive(Serialize, Deserialize)]
struct LoginRequest {
    redirect_url: String,
    state: CsrfToken,
    nonce: Nonce,
}

impl AuthenticatorService {
    pub async fn new(service: Arc<dyn HttpService + Send + Sync>, discovery_url: &str, client_id: &str, client_secret: &str) -> Result<Self, Error> {
        let http_client = Client::new();

        let provider_metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(discovery_url.to_string())?,
            get_client(&http_client)
        ).await?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string()))
        );

        Ok(Self {
            service,
            http_client,
            client,
            session_cookie: "session".to_string()
        })
    }
}

#[async_trait]
impl HttpService for AuthenticatorService {
    async fn call(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, Error>>, Error> {
        let http_ctx = req.extensions().get::<HttpContext>().ok_or("No HttpContext")?;

        let session_cookie = req.headers().get(header::COOKIE).and_then(|cookies|
            Cookie::split_parse(cookies.to_str().ok()?)
                .find_map(|c| c.ok().filter(|c| c.name() == self.session_cookie))
        );

        let session = match session_cookie {
            Some(session_cookie) => http_ctx.sessions.load_session(session_cookie.value().to_string()).await?,
            None => None
        }.and_then(|session| session.validate());

        //User is set when the user is logged in
        let user = Option::from(&session).and_then(|session: &Session| session.get::<String>("user"));
        if user.is_some() {
            return self.service.call(req).await;
        }

        let session = if let (Some(mut session), Some(query)) = (session, req.uri().query()) {
            // Parse the query string into key-value pairs
            let query_pairs: Vec<(Cow<'_, str>, Cow<'_, str>)> = form_urlencoded::parse(query.as_bytes()).collect();

            let login: Option<LoginRequest> = session.get("login");
            let state = query_pairs.iter().find(|(key, _)| *key == "state");
            let code = query_pairs.iter().find(|(key, _)| *key == "code");

            //If the user is logging in, redirect to the original page
            if let (Some(login), Some((_, state)), Some((_, code))) = (login, state, code) {
                if state.as_ref() != login.state.secret() {
                    return Err("Invalid state".into());
                }

                let code = AuthorizationCode::new(code.to_string());
                let request = self
                    .client
                    .exchange_code(code)
                    .set_redirect_uri(Cow::Owned(RedirectUrl::new(login.redirect_url.clone())?));

                let ret = request.request_async(get_client(&self.http_client)).await?;

                let user = ret.id_token().ok_or("No ID token")?.claims(&self.client.id_token_verifier(), &login.nonce)?;

                let body = BoxBody::new(Empty::new().map_err(|e| -> Error { Box::new(e) }));
                let resp = Response::builder()
                    .header(header::LOCATION, HeaderValue::from_str(&login.redirect_url)?)
                    .status(StatusCode::TEMPORARY_REDIRECT);

                session.remove("login");
                session.insert("user", user.email().ok_or("No email"))?;

                return Ok(resp.body(body)?);
            }

            Some(session)
        } else {
            None
        };

        //Redirect to the login page
        let mut uri_parts = req.uri().clone().into_parts();
        let authority = req.headers().get(header::HOST);
        uri_parts.scheme = Some(Scheme::HTTP);
        uri_parts.authority = uri_parts.authority.or_else(|| {
            authority
                .and_then(|x| x.to_str().ok())
                .and_then(|x| Authority::from_str(x).ok())
        });

        let uri = Uri::from_parts(uri_parts)?;
        let client = self.client.clone().set_redirect_uri(RedirectUrl::new(uri.to_string())?);

        let (redirect, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                || CsrfToken::new_random(),
                || Nonce::new_random(),
            )
            .url();

        let body = BoxBody::new(Empty::new().map_err(|e| -> Error { Box::new(e) }));
        let mut resp = Response::builder().header(header::LOCATION, redirect.to_string()).status(StatusCode::TEMPORARY_REDIRECT);

        let mut session = session.unwrap_or_default();
        session.insert("login", LoginRequest {
            redirect_url: uri.to_string(),
            state,
            nonce
        })?;

        let cookie = http_ctx.sessions.store_session(session).await?;

        if let Some(cookie) = cookie {
            resp = resp.header(header::COOKIE, HeaderValue::from_str(&cookie)?);
        }

        Ok(resp.body(body)?)
    }
}
