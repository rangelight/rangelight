#![allow(clippy::unwrap_used)]

use super::*;

fn headers(cookies: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for value in cookies {
        headers.append(header::COOKIE, HeaderValue::from_str(value).unwrap());
    }
    headers
}

#[test]
fn cookie_finds_the_named_value_in_any_header() {
    let both = headers(&["a=1; __Host-rl_session=tok", "__Host-rl_login=st"]);
    assert_eq!(cookie(&both, SESSION_COOKIE), Some("tok"));
    assert_eq!(cookie(&both, SIGN_IN_COOKIE), Some("st"));
    for missing in [
        &[][..],
        &["__Host-rl_session="],
        &["x__Host-rl_session=tok"],
        &["rl_session=tok"],
    ] {
        assert_eq!(
            cookie(&headers(missing), SESSION_COOKIE),
            None,
            "{missing:?}"
        );
    }
}

#[test]
fn login_cookies_are_host_only_http_only_secure_and_lax() {
    let value = set_cookie(SESSION_COOKIE, &"tok".to_owned().into(), ABSOLUTE).unwrap();
    assert!(value.is_sensitive());
    assert_eq!(
        value.to_str().unwrap(),
        "__Host-rl_session=tok; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800"
    );
    for clear in [CLEAR_SESSION, CLEAR_SIGN_IN] {
        assert!(
            clear
                .to_str()
                .unwrap()
                .ends_with("; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0")
        );
    }
}
