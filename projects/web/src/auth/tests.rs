use super::*;
use axum::http::HeaderValue;

fn cookie_header(set_cookie: &str) -> HeaderMap {
    // A browser sends back the name and value only.
    let pair = set_cookie.split(';').next().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, HeaderValue::from_str(pair).unwrap());
    headers
}

#[test]
fn the_token_signs_in_and_the_cookie_is_locked_down() {
    let auth = Auth::new().unwrap();
    let Login::Granted(set) = auth.login(&auth.token().to_owned(), "peer", true) else {
        panic!("the right token was refused");
    };
    for part in [
        "__Host-dusk=",
        "HttpOnly",
        "SameSite=Strict",
        "Secure",
        "Path=/",
    ] {
        assert!(set.contains(part), "{set} lacks {part}");
    }
    assert!(auth.signed_in(&cookie_header(&set)));
}

#[test]
fn a_wrong_token_or_a_forged_cookie_gets_nothing() {
    let auth = Auth::new().unwrap();
    assert!(matches!(
        auth.login("nope", "peer", true),
        Login::WrongToken
    ));
    assert!(!auth.signed_in(&HeaderMap::new()));
    assert!(!auth.signed_in(&cookie_header("__Host-dusk=forged")));
}

#[test]
fn logout_ends_the_session() {
    let auth = Auth::new().unwrap();
    let Login::Granted(set) = auth.login(&auth.token().to_owned(), "peer", false) else {
        panic!("the right token was refused");
    };
    let headers = cookie_header(&set);
    assert!(auth.signed_in(&headers));
    auth.logout(&headers, false);
    assert!(!auth.signed_in(&headers));
}

#[test]
fn failures_lock_the_peer_out_even_with_the_right_token() {
    let auth = Auth::new().unwrap();
    for _ in 0..5 {
        assert!(matches!(
            auth.login("nope", "peer", true),
            Login::WrongToken
        ));
    }
    let token = auth.token().to_owned();
    assert!(matches!(
        auth.login(&token, "peer", true),
        Login::Limited(_)
    ));
    assert!(matches!(
        auth.login(&token, "other", true),
        Login::Granted(_)
    ));
}
