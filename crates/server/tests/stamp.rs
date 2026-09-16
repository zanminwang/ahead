//! Pull copies the current record stamp from the scan row; an external
//! notification allocates one stamp per record and publishes it at that stamp.
use ahead_server::{Config, Host, host::HostRequest};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
};

fn run<T>(future: impl Future<Output = T>) -> T {
    let mut f = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(result) = f.as_mut().poll(&mut cx) {
            return result;
        }
    }
}
fn config() -> Config {
    Config::decode(json!({
        "schema":{"enums":[],"models":[{"name":"Entry","identity":["id"],"fields":[
            {"name":"id","nullable":false,"type":{"kind":"scalar","name":"string"}},
            {"name":"text","nullable":false,"type":{"kind":"scalar","name":"string"}}]}]},
        "loaders":["Entry"],
        "mutations":[]
    }))
    .unwrap()
}
/// `scan` returns the given rows; `publish` returns the given value. Both stay
/// raw `Value`s: these tests feed the engine answers the contract refuses.
struct Fixed {
    scan: Value,
    publish: Value,
    published: Mutex<Vec<HostRequest>>,
    loaded: Mutex<Vec<u64>>,
    advanced: Mutex<Vec<String>>,
    ensured: Mutex<Vec<String>>,
}
impl Fixed {
    fn new(scan: Value, publish: Value) -> Self {
        Self {
            scan,
            publish,
            published: Mutex::new(vec![]),
            loaded: Mutex::new(vec![]),
            advanced: Mutex::new(vec![]),
            ensured: Mutex::new(vec![]),
        }
    }
}
impl Host for Fixed {
    fn call(
        &self,
        r: Value,
    ) -> Pin<Box<dyn Future<Output = ahead_server::HostResult<Value>> + Send + '_>> {
        Box::pin(async move {
            let request: HostRequest = serde_json::from_value(r)
                .map_err(|error| format!("unsupported host request: {error}"))?;
            Ok(match &request {
                HostRequest::Head { .. } => json!(5),
                HostRequest::Scan { .. } => self.scan.clone(),
                HostRequest::Load { version, .. } => {
                    self.loaded.lock().unwrap().push(*version);
                    json!([{"id":"e","text":"t"}])
                }
                HostRequest::AdvanceStamp { identity_key, .. } => {
                    self.advanced.lock().unwrap().push(identity_key.clone());
                    json!(9)
                }
                HostRequest::EnsureStamp { identity_key, .. } => {
                    self.ensured.lock().unwrap().push(identity_key.clone());
                    json!(9)
                }
                HostRequest::Publish { .. } => {
                    self.published.lock().unwrap().push(request.clone());
                    self.publish.clone()
                }
                other => return Err(format!("unsupported {}", other.label())),
            })
        })
    }
}
fn pull_body() -> Vec<u8> {
    pull_body_declaring(&[("Entry", 1)])
}
/// A pull on `a` from cursor 0 declaring these read contracts.
fn pull_body_declaring(models: &[(&str, u64)]) -> Vec<u8> {
    ahead_core::PullRequest {
        client_id: "c".into(),
        channel: "a".into(),
        from_cursor: 0,
        models: models
            .iter()
            .map(|(name, version)| ((*name).to_string(), *version))
            .collect(),
    }
    .encode()
    .unwrap()
}
fn row(stamp: Value) -> Value {
    let mut row = json!({"channel":"a","cursor":1,"model":"Entry","identity":{"id":"e"},"identityKey":"{\"id\":\"e\"}"});
    if !stamp.is_null() {
        row["stamp"] = stamp;
    }
    json!([row])
}

#[test]
fn pull_copies_the_row_stamp_into_the_change() {
    let host = Fixed::new(row(json!(7)), Value::Null);
    let text = run(ahead_server::process_pull(
        &config(),
        "u",
        &pull_body(),
        &host,
    ))
    .unwrap();
    let page = ahead_core::PullPage::decode(text.as_bytes()).unwrap();
    assert_eq!(page.changes[0].stamp, 7);
    assert_eq!(
        *host.loaded.lock().unwrap(),
        [1],
        "the load names the model version it serves"
    );
}

#[test]
fn pull_normalizes_loader_rows_with_the_retained_contract_of_the_served_version() {
    // The retained contract of the served version, not the current schema,
    // decides which fields a loader may return.
    let host = Fixed::new(row(json!(7)), Value::Null);
    let mut c = json!({
        "schema":{"enums":[],"models":[{"name":"Entry","version":2,"identity":["id"],"fields":[
            {"name":"id","nullable":false,"type":{"kind":"scalar","name":"string"}},
            {"name":"text","nullable":false,"type":{"kind":"scalar","name":"string"}},
            {"name":"note","nullable":true,"type":{"kind":"scalar","name":"string"}}]}]},
        "loaders":["Entry"],
        "mutations":[]
    });
    c["models"] = json!([
        {"name":"Entry","version":1,"identity":["id"],"enums":[],"fields":[
            {"name":"id","nullable":false,"type":{"kind":"scalar","name":"string"}},
            {"name":"text","nullable":false,"type":{"kind":"scalar","name":"string"}}]},
        {"name":"Entry","version":2,"identity":["id"],"enums":[],"fields":c["schema"]["models"][0]["fields"].clone()}
    ]);
    let config = Config::decode(c).unwrap();
    // An old client declares v1: the v1 loader runs and the v1 contract shapes the row.
    let text = run(ahead_server::process_pull(
        &config,
        "u",
        &pull_body_declaring(&[("Entry", 1)]),
        &host,
    ))
    .unwrap();
    let page = ahead_core::PullPage::decode(text.as_bytes()).unwrap();
    assert_eq!(page.changes[0].state, json!({"text":"t"}));
    // A new client declares v2 for the same data: the v2 loader and contract.
    let text = run(ahead_server::process_pull(
        &config,
        "u",
        &pull_body_declaring(&[("Entry", 2)]),
        &host,
    ))
    .unwrap();
    let page = ahead_core::PullPage::decode(text.as_bytes()).unwrap();
    assert_eq!(page.changes[0].state, json!({"text":"t","note":null}));
    assert_eq!(
        *host.loaded.lock().unwrap(),
        [1, 2],
        "each pull reaches the loader of the version it declared"
    );
    // A declared version that is not retained, or a model this backend does
    // not have, is refused before anything is scanned or loaded.
    for (models, detail) in [
        (&[("Entry", 3)][..], json!({"model":"Entry","version":3})),
        (
            &[("Entry", 2), ("Ghost", 1)][..],
            json!({"model":"Ghost","version":1}),
        ),
    ] {
        let err = run(ahead_server::process_pull(
            &config,
            "u",
            &pull_body_declaring(models),
            &host,
        ))
        .unwrap_err();
        assert_eq!(
            err.code,
            ahead_server::code::MODEL_VERSION_UNSUPPORTED,
            "{err}"
        );
        assert_eq!(err.details, detail, "{err}");
    }
    assert_eq!(
        *host.loaded.lock().unwrap(),
        [1, 2],
        "a refused declaration loads nothing"
    );
}

#[test]
fn a_page_holding_a_model_the_client_did_not_declare_is_refused_whole() {
    // Pending per-read isolation (#95): the pull is refused as a whole, with
    // the model named, rather than skipped or served at a guessed version.
    let host = Fixed::new(row(json!(7)), Value::Null);
    let c = json!({
        "schema":{"enums":[],"models":[
            {"name":"Entry","identity":["id"],"fields":[
                {"name":"id","nullable":false,"type":{"kind":"scalar","name":"string"}},
                {"name":"text","nullable":false,"type":{"kind":"scalar","name":"string"}}]},
            {"name":"Note","identity":["id"],"fields":[
                {"name":"id","nullable":false,"type":{"kind":"scalar","name":"string"}}]}]},
        "loaders":["Entry","Note"],
        "mutations":[]
    });
    let config = Config::decode(c).unwrap();
    let err = run(ahead_server::process_pull(
        &config,
        "u",
        &pull_body_declaring(&[("Note", 1)]),
        &host,
    ))
    .unwrap_err();
    assert_eq!(
        err.code,
        ahead_server::code::MODEL_VERSION_UNSUPPORTED,
        "{err}"
    );
    assert_eq!(err.details, json!({"model":"Entry"}));
    assert!(host.loaded.lock().unwrap().is_empty(), "no loader ran");
}

#[test]
fn pull_rejects_rows_without_a_positive_stamp() {
    for bad in [Value::Null, json!(0), json!(-1), json!(9007199254740992u64)] {
        let host = Fixed::new(row(bad.clone()), Value::Null);
        let err = run(ahead_server::process_pull(
            &config(),
            "u",
            &pull_body(),
            &host,
        ))
        .unwrap_err();
        assert!(err.message.contains("stamp"), "{bad}: {err}");
        assert_eq!(err.code, ahead_server::code::STORAGE_INVALID);
    }
}

#[test]
fn an_external_settlement_advances_one_stamp_per_record_and_distributes_it_at_that_stamp() {
    let settlement = json!({
        "changes":[{"model":"Entry","identity":{"id":"e"}}],
        "publications":[{"channel":"a"},{"channel":"b"}]
    });
    let ok = Fixed::new(json!([]), json!({"cursor":3,"stamp":9}));
    let answer = run(ahead_server::settle_external(&config(), &settlement, &ok)).unwrap();
    assert_eq!(
        answer,
        json!([{"model":"Entry","identity":{"id":"e"},"stamp":9}]),
        "the changed records come back with their stamps"
    );
    let published = ok.published.lock().unwrap();
    assert_eq!(published.len(), 2, "one invalidation per channel");
    for request in published.iter() {
        let HostRequest::Publish { stamp, .. } = request else {
            panic!("not a publish");
        };
        assert_eq!(*stamp, 9, "both channels carry the one allocated stamp");
    }
    drop(published);
    // A publication-only record keeps its stamp; `ensureStamp` initializes it.
    let ensure = Fixed::new(json!([]), json!({"cursor":4,"stamp":9}));
    let publication_only = json!({
        "changes":[],
        "publications":[{"channel":"a","records":[{"model":"Entry","identity":{"id":"e"}}]}]
    });
    run(ahead_server::settle_external(
        &config(),
        &publication_only,
        &ensure,
    ))
    .unwrap();
    assert_eq!(
        ensure.ensured.lock().unwrap().len(),
        1,
        "an unchanged member is initialized, not advanced"
    );
    assert!(ensure.advanced.lock().unwrap().is_empty());
    // The host must echo the stamp the engine named; anything else is unusable.
    for bad in [
        json!(3),
        json!({"cursor":3}),
        json!({"cursor":3,"stamp":0}),
        json!({"cursor":3,"stamp":8}),
    ] {
        let host = Fixed::new(json!([]), bad.clone());
        let err = run(ahead_server::settle_external(&config(), &settlement, &host)).unwrap_err();
        assert_eq!(err.code, ahead_server::code::HOST_INVALID, "{bad}: {err}");
    }
    // A rejection or a malformed settlement is refused before any host call.
    for bad in [
        json!({"rejection":"x"}),
        json!({"changes":[]}),
        json!({"channels":["a"]}),
    ] {
        let host = Fixed::new(json!([]), json!({"cursor":3,"stamp":9}));
        let err = run(ahead_server::settle_external(&config(), &bad, &host)).unwrap_err();
        assert_eq!(
            err.code,
            ahead_server::code::PUBLISH_INVALID,
            "{bad}: {err}"
        );
        assert!(host.published.lock().unwrap().is_empty());
    }
}

#[test]
fn live_negotiation_establishes_current_heads_and_rejects_cursor_modes() {
    let host = Fixed::new(json!([]), Value::Null);
    let result = run(ahead_server::live::negotiate(
        &config(),
        "u",
        br#"{"type":"subscribe","scopes":["a"],"models":{"Entry":1}}"#,
        &host,
    ))
    .unwrap();
    assert_eq!(result.subscriptions[0].from_cursor, 5);
    assert_eq!(
        result.models.get("Entry"),
        Some(&1),
        "the session keeps the declaration"
    );
    // The declaration is checked at the handshake, like a pull's.
    for (frame, code) in [
        (
            r#"{"type":"subscribe","scopes":["a"]}"#,
            ahead_server::code::REQUEST_INVALID,
        ),
        (
            r#"{"type":"subscribe","scopes":["a"],"models":{"Entry":2}}"#,
            ahead_server::code::MODEL_VERSION_UNSUPPORTED,
        ),
        (
            r#"{"type":"subscribe","scopes":["a"],"models":{"Ghost":1}}"#,
            ahead_server::code::MODEL_VERSION_UNSUPPORTED,
        ),
    ] {
        let err = run(ahead_server::live::negotiate(
            &config(),
            "u",
            frame.as_bytes(),
            &host,
        ))
        .unwrap_err();
        assert_eq!(err.code, code, "{frame}: {err}");
    }
    for cursors in [
        json!({"a":0}),
        json!({}),
        json!({"a":6}),
        json!({"a":-1}),
        json!({"a":1.5}),
        json!({"a":"0"}),
        json!({"a":null}),
        json!({"a":9007199254740992u64}),
        json!({"a":0,"b":0}),
        json!(null),
        json!([]),
    ] {
        let request =
            json!({"type":"subscribe","scopes":["a"],"models":{"Entry":1},"cursors":cursors});
        assert!(
            run(ahead_server::live::negotiate(
                &config(),
                "u",
                request.to_string().as_bytes(),
                &host
            ))
            .is_err(),
            "accepted {request}"
        );
    }
}
