//! HTTP for sign-in: the login and logout routes, and the gate every other
//! API request passes while sign-in is configured.

use super::service::{Auth, AuthError, SESSION_LIFETIME};
use crate::app::{AppState, is_loopback_host};
use crate::config::IpNet;
use crate::respond::{ApiError, ApiResult};
use axum::Extension;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub const SESSION_COOKIE: &str = "harmony_session";
const LOGIN: &str = "/api/v1/auth/login";
const LOGOUT: &str = "/api/v1/auth/logout";

/// Who the gate let in, for handlers that need the user.
#[derive(Clone, Debug)]
pub struct SignedIn {
    pub user_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(LOGIN, post(login))
        .route(LOGOUT, post(logout))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    username: String,
    password: String,
}

fn disabled() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "auth_disabled",
        "sign-in is not configured on this Music Server",
    )
}

async fn login(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    let Some(auth) = &state.auth else {
        return Err(disabled().into_response());
    };
    let request: LoginRequest = serde_json::from_slice(&body).map_err(|_| {
        ApiError::bad_request("send {\"username\", \"password\"} as JSON").into_response()
    })?;
    if !(1..=256).contains(&request.username.chars().count())
        || !(1..=1024).contains(&request.password.chars().count())
    {
        return Err(ApiError::bad_request(
            "username must be 1 to 256 characters and password 1 to 1024",
        )
        .into_response());
    }
    let peer = peer_ip(peer.as_ref().map(|Extension(info)| info));
    let client = client_ip(peer, &headers, auth.trusted_proxies());
    let token = auth
        .login(client, &request.username, &request.password)
        .await
        .map_err(refusal)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, SESSION_LIFETIME.as_secs(), &headers),
    );
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let Some(auth) = &state.auth else {
        return Err(disabled());
    };
    if let Some(token) = cookie_token(&headers) {
        auth.logout(token).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, session_cookie("", 0, &headers));
    Ok(response)
}

/// Lets an API request through only with a session cookie or Basic
/// credentials. Health, login and logout stay open, and so does everything
/// outside `/api/`: the Web Client's files and the docs. No answer carries
/// `WWW-Authenticate`, which would open the browser's own password dialog.
pub async fn gate(auth: &Auth, mut request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let method = request.method();
    let open = !path.starts_with("/api/")
        || (path == "/api/v1/health" && matches!(*method, Method::GET | Method::HEAD))
        || ((path == LOGIN || path == LOGOUT) && method == Method::POST);
    if open {
        return next.run(request).await;
    }
    let headers = request.headers();
    let mut renewed = None;
    let mut user_id = None;
    if let Some(token) = cookie_token(headers) {
        match auth.session(token).await {
            Ok(Some(session)) => {
                if session.renewed {
                    renewed = Some(session_cookie(token, SESSION_LIFETIME.as_secs(), headers));
                }
                user_id = Some(session.user_id);
            }
            Ok(None) => {}
            Err(error) => return ApiError::internal(error).into_response(),
        }
    }
    if user_id.is_none()
        && let Some((header, username, password)) = basic_credentials(headers)
    {
        let peer = peer_ip(request.extensions().get());
        let client = client_ip(peer, headers, auth.trusted_proxies());
        match auth.basic(client, header, &username, &password).await {
            Ok(id) => user_id = Some(id),
            Err(error) => return refusal(error),
        }
    }
    let Some(user_id) = user_id else {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "sign in to use the Music Server",
        )
        .into_response();
    };
    request.extensions_mut().insert(SignedIn { user_id });
    let mut response = next.run(request).await;
    if let Some(cookie) = renewed {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    response
}

fn refusal(error: AuthError) -> Response {
    match error {
        AuthError::TooManyAttempts(seconds) => {
            let mut response = ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
                format!("too many failed sign-in attempts; try again in {seconds} s"),
            )
            .into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
            response
        }
        AuthError::InvalidCredentials => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "wrong username or password",
        )
        .into_response(),
        AuthError::Storage(error) => ApiError::internal(error).into_response(),
    }
}

/// `Secure` unless the browser reached a loopback host, where plain HTTP is
/// the norm and a Secure cookie would never come back.
fn session_cookie(token: &str, max_age: u64, headers: &HeaderMap) -> HeaderValue {
    let secure = !headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(is_loopback_host);
    let value = format!(
        "{SESSION_COOKIE}={token}; Max-Age={max_age}; Path=/; HttpOnly; SameSite=Lax{}",
        if secure { "; Secure" } else { "" }
    );
    HeaderValue::try_from(value).expect("cookie header")
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == SESSION_COOKIE && !value.is_empty())
        .map(|(_, value)| value)
}

/// The raw header and its decoded username and password.
fn basic_credentials(headers: &HeaderMap) -> Option<(&str, String, String)> {
    let header = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = String::from_utf8(STANDARD.decode(encoded.trim()).ok()?).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((header, username.to_owned(), password.to_owned()))
}

fn peer_ip(peer: Option<&ConnectInfo<SocketAddr>>) -> Option<IpAddr> {
    peer.map(|ConnectInfo(address)| address.ip())
}

/// The address the attempt limits count: the socket's peer, unless that is a
/// trusted proxy. Then `X-Forwarded-For` is read from the right, where each
/// proxy appended the address it saw, and the first address that is not a
/// trusted proxy is the client. Anything to its left, the client could have
/// written itself. Without a peer (a test calling the router directly) every
/// request is one client.
pub fn client_ip(peer: Option<IpAddr>, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    let Some(peer) = peer.map(|peer| peer.to_canonical()) else {
        return IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    };
    let is_trusted = |address: IpAddr| trusted.iter().any(|net| net.contains(address));
    if !is_trusted(peer) {
        return peer;
    }
    let forwarded: Vec<String> = headers
        .get_all("x-forwarded-for")
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .collect();
    let mut client = peer;
    for hop in forwarded.iter().flat_map(|value| value.split(',')).rev() {
        // An entry that is no address ends the walk at the last proxy.
        let Ok(address) = hop.trim().parse::<IpAddr>() else {
            break;
        };
        client = address.to_canonical();
        if !is_trusted(client) {
            break;
        }
    }
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(forwarded: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in forwarded {
            headers.append("x-forwarded-for", HeaderValue::from_static(value));
        }
        headers
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("address")
    }

    #[test]
    fn forwarded_addresses_count_only_from_a_trusted_proxy() {
        let trusted = [IpNet::parse("172.18.0.0/16").unwrap()];
        let spoofed = headers(&["1.2.3.4"]);
        // An untrusted peer is the client, whatever it forwards.
        assert_eq!(
            client_ip(Some(ip("203.0.113.9")), &spoofed, &trusted),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(Some(ip("172.18.0.2")), &spoofed, &trusted),
            ip("1.2.3.4")
        );
        // The rightmost untrusted entry wins over what the client wrote.
        let chained = headers(&["6.6.6.6, 198.51.100.4", "172.18.0.3"]);
        assert_eq!(
            client_ip(Some(ip("172.18.0.2")), &chained, &trusted),
            ip("198.51.100.4")
        );
        // Only proxies: the leftmost one is as far as the chain goes.
        let proxies = headers(&["172.18.0.9 , 172.18.0.3"]);
        assert_eq!(
            client_ip(Some(ip("172.18.0.2")), &proxies, &trusted),
            ip("172.18.0.9")
        );
        let garbage = headers(&["198.51.100.4, unknown, 172.18.0.3"]);
        assert_eq!(
            client_ip(Some(ip("172.18.0.2")), &garbage, &trusted),
            ip("172.18.0.3")
        );
        assert_eq!(
            client_ip(Some(ip("172.18.0.2")), &HeaderMap::new(), &trusted),
            ip("172.18.0.2")
        );
        assert_eq!(
            client_ip(Some(ip("::ffff:203.0.113.9")), &spoofed, &trusted),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(None, &spoofed, &trusted),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn reads_the_session_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; harmony_session=abc; other=1"),
        );
        assert_eq!(cookie_token(&headers), Some("abc"));
        headers.insert(header::COOKIE, HeaderValue::from_static("harmony_session="));
        assert_eq!(cookie_token(&headers), None);
    }
}
