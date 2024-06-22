use std::borrow::Cow;
use std::error;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

use async_once_cell::OnceCell;
use async_session::{SessionStore, Session};
use async_trait::async_trait;

use cookie::Cookie;

use hyper::body::{Bytes, Incoming};
use hyper::header::HeaderValue;
use hyper::http::uri::{Authority, InvalidUri, Scheme};
use hyper::{header, Request, Response, StatusCode, Uri};

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::url::ParseError;
use openidconnect::{
    AsyncHttpClient, AuthorizationCode, ClaimsVerificationError, ClientId, ClientSecret, ConfigurationError, CsrfToken, DiscoveryError, EndpointMaybeSet, EndpointNotSet, EndpointSet, ErrorResponse, HttpRequest, HttpResponse, IssuerUrl, Nonce, RedirectUrl, RequestTokenError, Scope, TokenResponse
};

use serde_derive::{Deserialize, Serialize};

use crate::error::Error;
use crate::handler::Context;
use crate::http::HttpService;

use super::{Client, HttpContext, HttpError};

type OIDCClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

pub struct AuthenticatorService {
    discovery_url: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    pub service: Arc<dyn HttpService + Send + Sync>,
    pub http_client: Client,
    pub client: OnceCell<OIDCClient>,
    pub session_cookie: String
}

#[derive(Deserialize)]
pub struct Token {}

#[derive(Debug)]
pub enum ClientError {
    Uri(InvalidUri),
    Other(&'static str)
}

impl error::Error for ClientError {}

impl Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Upstream server error")
    }
}

impl<'c> AsyncHttpClient<'c> for Client {
    type Error = ClientError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'c>>;

    fn call(&'c self, req: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let mut request = Request::builder()
                .header(header::HOST, req.uri().host().ok_or(ClientError::Other("No hostname"))?)
                .method(req.method()).uri(req.uri());
                

            for (key, value) in req.headers() {
                request = request.header(key.as_str(), value.as_bytes());
            }

            let request = request
                .body(BoxBody::new(
                    Full::new(Bytes::from(req.body().clone())).map_err(From::from),
                )).map_err(|_| ClientError::Other("Can't create response"))?;

            let mut conn = self.get_connection(req.uri()).await.map_err(|_| ClientError::Other("No connection"))?;
            let resp = conn.send_request(request).await.map_err(|_| ClientError::Other("Can't send request"))?;

            let mut builder = Response::builder().status(resp.status());
            for (key, value) in resp.headers() {
                builder = builder.header(key, value);
            }

            let body = resp.collect().await.map_err(|_| ClientError::Other("Can't receive body"))?;
            builder.body(body.to_bytes().to_vec()).map_err(|_| ClientError::Other("Can convert body"))
        })
    }
}

impl From<ParseError> for HttpError {
    fn from(error: ParseError) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<async_session::Error> for HttpError {
    fn from(error: async_session::Error) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<async_session::serde_json::Error> for HttpError {
    fn from(error: async_session::serde_json::Error) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<ClaimsVerificationError> for HttpError {
    fn from(error: ClaimsVerificationError) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<ClientError> for HttpError {
    fn from(error: ClientError) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<ConfigurationError> for HttpError {
    fn from(error: ConfigurationError) -> Self {
        HttpError::String(error.to_string())
    }
}

impl<T: std::error::Error, U: ErrorResponse> From<RequestTokenError<T, U>> for HttpError {
    fn from(error: RequestTokenError<T, U>) -> Self {
        HttpError::String(error.to_string())
    }
}

impl<T: std::error::Error> From<DiscoveryError<T>> for HttpError {
    fn from(error: DiscoveryError<T>) -> Self {
        HttpError::String(error.to_string())
    }
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

        Ok(Self {
            discovery_url: IssuerUrl::new(discovery_url.to_string())?,
            client_id: ClientId::new(client_id.to_string()),
            client_secret: ClientSecret::new(client_secret.to_string()),
            service,
            http_client,
            client: OnceCell::new(),
            session_cookie: "session".to_string()
        })
    }
}

#[async_trait]
impl HttpService for AuthenticatorService {
    async fn call(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, HttpError>>, HttpError> {
        let http_ctx = req.extensions().get::<HttpContext>().ok_or("No HttpContext")?;
        let ctx = req.extensions().get::<Context>().ok_or("No Context")?;

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

        //Lazy initialize the client, so we don't fail if the upstream server is down
        let client = self.client.get_or_try_init(async {
            let provider_metadata = CoreProviderMetadata::discover_async(
                IssuerUrl::new(self.discovery_url.to_string())?,
                &self.http_client
            ).await?;
    
            Ok::<_, HttpError>(CoreClient::from_provider_metadata(provider_metadata, self.client_id.clone(), Some(self.client_secret.clone())))
        }).await?;

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
                let request = client
                    .exchange_code(code)?
                    .set_redirect_uri(Cow::Owned(RedirectUrl::new(login.redirect_url.clone())?));

                let ret = request.request_async(&self.http_client).await?;

                let user = ret.id_token().ok_or("No ID token")?.claims(&client.id_token_verifier(), &login.nonce)?;

                let body = BoxBody::new(Empty::new().map_err(From::from));
                let resp = Response::builder()
                    .header(header::LOCATION, HeaderValue::from_str(&login.redirect_url)?)
                    .status(StatusCode::TEMPORARY_REDIRECT);

                session.remove("login");
                session.insert("user", user.email().ok_or("No email")?)?;

                return Ok(resp.body(body)?);
            }

            Some(session)
        } else {
            None
        };

        //Redirect to the login page
        let mut uri_parts = req.uri().clone().into_parts();
        let authority = req.headers().get(header::HOST);
        uri_parts.scheme = if ctx.secure { Scheme::HTTPS } else { Scheme::HTTP }.into();
        uri_parts.authority = uri_parts.authority.or_else(|| {
            authority
                .and_then(|x| x.to_str().ok())
                .and_then(|x| Authority::from_str(x).ok())
        });

        let uri = Uri::from_parts(uri_parts)?;
        let client = client.clone().set_redirect_uri(RedirectUrl::new(uri.to_string())?);

        let (redirect, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                || CsrfToken::new_random(),
                || Nonce::new_random(),
            )
            .add_scope(Scope::new("email".to_string()))
            .url();

        let body = BoxBody::new(Empty::new().map_err(From::from));
        let mut resp = Response::builder().header(header::LOCATION, redirect.to_string()).status(StatusCode::TEMPORARY_REDIRECT);

        let mut session = session.unwrap_or_default();
        session.insert("login", LoginRequest {
            redirect_url: uri.to_string(),
            state,
            nonce
        })?;

        let cookie = http_ctx.sessions.store_session(session).await?;

        if let Some(cookie) = cookie {
            let mut cookie_header = vec!(format!("{}={}", self.session_cookie, cookie));
            cookie_header.push("HttpOnly".to_string());
            cookie_header.push(format!("Path={}", "/"));
            if ctx.secure {
                cookie_header.push("Secure".to_string());
            }
            resp = resp.header(header::SET_COOKIE, HeaderValue::from_str(&cookie_header.join("; "))?);
        }

        Ok(resp.body(body)?)
    }
}
