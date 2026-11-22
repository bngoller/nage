//! The page for an unknown address (SPEC 5.3, INV-27, decisions 102, 109 and 118): the
//! router's fallback behind the same layers as every route, and the bare answers that stay
//! bare.

mod common;

use axum::http::StatusCode;
use nage_web::messages::Refusal;

use common::app::{ADMIN, App, PASSWORD, Req, assert_security_headers, main_of};
use common::start_tags;

/// With a valid session, `GET` of an address that no route answers gets 404 with a page on
/// the layout: the title `Nage`, the heading, the message that says in three parts that Nage
/// has no page at that address, and a button to the home page; the four response headers
/// are there, `Cache-Control: no-store` among them. `/static` without its slash is such an
/// address too.
///
/// Proves: decision 118 (a request with a valid session for an address no route answers gets
/// 404 with a page that explains in three parts and offers the home page), decision 102,
/// decision 73, decisions 40 and 119 (the headers on the fallback), decision 109 (`/static`
/// without the slash is like any other address).
#[tokio::test]
async fn an_unknown_address_gets_the_page_of_the_404() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for uri in ["/no-such-page", "/accounts/7/history?tab=owner", "/static"] {
        let page = client.get(uri).await;
        assert_eq!(page.status, StatusCode::NOT_FOUND, "GET {uri} status");
        assert_eq!(
            page.header("content-type"),
            Some("text/html; charset=utf-8"),
            "GET {uri} content-type"
        );
        assert_security_headers(&page, uri);
        let html = &page.body;
        assert!(
            html.contains("<title>Nage</title>"),
            "the title of the page for {uri}:\n{html}"
        );
        assert!(
            html.contains("<h1>Page not found</h1>"),
            "the heading of the page for {uri}:\n{html}"
        );
        assert!(
            page.shows(&Refusal::NotFound.to_string()),
            "the page for {uri} does not say that Nage has no page there:\n{html}"
        );
        let main = main_of(html);
        let forms = start_tags(main, "<form");
        assert_eq!(forms.len(), 1, "forms on the page for {uri}: {forms:?}");
        assert!(
            forms[0].contains(r#"method="get""#) && forms[0].contains(r#"action="/""#),
            "the form of the page for {uri} does not lead to the home page: {}",
            forms[0]
        );
        assert!(
            main.contains(r#"<button type="submit">Go to the home page</button>"#),
            "the page for {uri} has no button to the home page:\n{main}"
        );
    }
}

/// The page never prints the address it was asked for: neither the path nor the query, as
/// sent or decoded, appears anywhere in it, so a link from someone else cannot put its own
/// words on a page of Nage.
///
/// Proves: decision 118 (the page never prints the address).
#[tokio::test]
async fn the_page_for_an_unknown_address_never_shows_the_address() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client
        .get("/zzyzx-path/%3Cb%3Ezzyzx-bold%3C/b%3E?zzyzx-key=zzyzx-value&say=Your%20account%20is%20locked")
        .await;
    assert_eq!(page.status, StatusCode::NOT_FOUND, "status");
    for absent in ["zzyzx", "%3C", "<b>", "locked", "Your account"] {
        assert!(
            !page.body.contains(absent),
            "the page holds {absent:?} of the address it was asked for:\n{}",
            page.body
        );
    }
}

/// The fallback stands behind the same layers as every route. With a session, a POST to an
/// unknown address from one of Nage's own pages gets the 404 page; from another site it is
/// refused by the Origin check with 403 first; with a declared body over 64 KiB it is
/// refused by the body limit with 413 first. Without a session an unknown address still
/// leads to the sign-in page, a GET with `next` and a POST without; and while no user
/// exists, to `/setup`.
///
/// Proves: decision 118 (the page is the router's fallback, behind the same layers as every
/// route, so the body limit, the Origin check and the session rule cover it), INV-27 (an
/// unknown address needs a session), decisions 109 and 110.
#[tokio::test]
async fn the_fallback_stands_behind_the_body_limit_the_origin_check_and_the_session_rule() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let posted = client.post("/no-such-page", &[("a", "b")]).await;
    assert_eq!(
        posted.status,
        StatusCode::NOT_FOUND,
        "POST /no-such-page with a session"
    );
    assert!(
        posted.shows(&Refusal::NotFound.to_string()),
        "a POST to an unknown address does not get the page:\n{}",
        posted.body
    );
    let cross_site = client
        .send(Req::form("/no-such-page", &[]).header("origin", "http://elsewhere.test"))
        .await;
    assert_eq!(
        cross_site.status,
        StatusCode::FORBIDDEN,
        "a cross-site POST to an unknown address"
    );
    let too_large = client
        .send(Req::form("/no-such-page", &[]).header("content-length", "65537"))
        .await;
    assert_eq!(
        too_large.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "a POST to an unknown address that declares 65,537 bytes"
    );

    let anonymous = app.client().get("/no-such-page?x=1").await;
    assert_eq!(
        anonymous.status,
        StatusCode::SEE_OTHER,
        "GET of an unknown address without a session"
    );
    assert_eq!(
        anonymous.location(),
        Some("/login?next=%2Fno-such-page%3Fx%3D1"),
        "where an unknown address leads without a session"
    );
    assert!(
        !anonymous.shows(&Refusal::NotFound.to_string()),
        "the answer without a session holds the page:\n{}",
        anonymous.body
    );
    let anonymous_post = app.client().post("/no-such-page", &[]).await;
    assert_eq!(
        anonymous_post.location(),
        Some("/login"),
        "where a POST to an unknown address leads without a session"
    );

    let fresh = App::start().await;
    let before_set_up = fresh.client().get("/no-such-page").await;
    assert_eq!(
        before_set_up.status,
        StatusCode::SEE_OTHER,
        "GET of an unknown address while no user exists"
    );
    assert_eq!(
        before_set_up.location(),
        Some("/setup"),
        "where an unknown address leads while no user exists"
    );
}

/// The answers that have no page keep none: an unknown path under `/static/`, and `/static/`
/// itself, answer a bare 404, with a session and without, since nobody opens an asset by
/// hand; and a known address asked with a method it does not take, `/logout` typed into the
/// address bar, answers a bare 405 that names the method it takes.
///
/// Proves: decision 118 (an unknown path under `/static/` keeps its bare 404 and a wrong
/// method on a known address its bare 405), decision 46, SPEC 5.3 (logout is a POST).
#[tokio::test]
async fn the_answers_without_a_page_stay_bare() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for uri in ["/static/nope.css", "/static/"] {
        for (who, reply) in [
            ("with a session", client.get(uri).await),
            ("without a session", app.client().get(uri).await),
        ] {
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "GET {uri} {who}");
            assert_eq!(reply.body, "", "the body of the 404 of {uri} {who}");
        }
    }
    let wrong_method = client.get("/logout").await;
    assert_eq!(
        wrong_method.status,
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /logout with a session"
    );
    assert_eq!(wrong_method.body, "", "the body of the 405");
    assert_eq!(
        wrong_method.header("allow"),
        Some("POST"),
        "the Allow header of the 405"
    );
    assert_eq!(
        app.session_rows().await,
        1,
        "sessions after GET /logout: it ends nothing"
    );
}
