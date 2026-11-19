use super::proof::{hex, hmac_sha256};
use super::*;

#[test]
fn hmac_matches_rfc_4231_case_2() {
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    assert_eq!(
        hex(&mac),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn the_two_proofs_differ_and_verify() {
    let k = [7u8; 32];
    let (nc, ns) = ("aa".repeat(32), "bb".repeat(32));
    let core = core_proof(&k, "key", &nc, &ns);
    let client = client_proof(&k, "key", &nc, &ns);
    assert_ne!(core, client);
    assert_eq!(core.len(), 64);
    assert!(proof_matches(&core, &core_proof(&k, "key", &nc, &ns)));
    assert!(!proof_matches(&core, &client));
    assert!(!proof_matches(&core, &core_proof(&k, "key", &ns, &nc)));
    assert!(!proof_matches(
        &core,
        &core_proof(&[8u8; 32], "key", &nc, &ns)
    ));
    assert!(!proof_matches(&core, ""));
}

#[test]
fn a_token_key_is_the_sha256_of_its_secret() {
    assert_eq!(
        hex(&token_key("abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn nonces_are_fresh_lowercase_hex() {
    let (one, two) = (nonce(), nonce());
    assert_eq!(one.len(), 64);
    assert!(one.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    assert_ne!(one, two);
}

#[test]
fn a_version_outside_the_range_is_refused() {
    let mut hello = Hello::new("bot");
    for (protocol, expected) in [
        (35, Err(Refusal::Version)),
        (36, Ok(())),
        (37, Err(Refusal::Version)),
    ] {
        hello.protocol = protocol;
        assert_eq!(hello.accepted(), expected, "protocol {protocol}");
    }
}

#[test]
fn a_name_with_a_newline_is_refused() {
    let long = "a".repeat(41);
    for name in ["bot\n", "", long.as_str(), "bot\u{7}", "bót"] {
        assert_eq!(
            Hello::new(name).accepted(),
            Err(Refusal::BadName),
            "{name:?}"
        );
    }
    assert_eq!(Hello::new("review-bot 2.1_x").accepted(), Ok(()));
    assert_eq!(Hello::new(&"a".repeat(40)).accepted(), Ok(()));
}

#[test]
fn a_hello_without_auth_or_nonce_still_parses() {
    let hello: Hello = serde_json::from_str(r#"{"protocol":36,"client":"tui"}"#).unwrap();
    assert_eq!(hello, Hello::new("tui"));
}

/// These literals are the frozen wire format: two versions of duskcode say them to each
/// other before either knows the other. Never change one; add a field with
/// `#[serde(default)]` and a new literal instead.
#[test]
fn the_handshake_frames_are_pinned() {
    let core = CoreInfo {
        protocol: 36,
        oldest: 36,
        version: "0.77.0".to_owned(),
        owner: OwnerKind::Gui,
        busy: Busy {
            clients: 2,
            running: 1,
        },
    };
    let hello = Hello {
        protocol: 36,
        client: "bot".to_owned(),
        auth: Some(Claim::Token {
            id: "q7m2k9xw3a".to_owned(),
        }),
        nonce: "cc".to_owned(),
    };
    let key = Hello {
        auth: Some(Claim::Key),
        ..Hello::new("tui")
    };
    let proof = Proof {
        proof: "dd".to_owned(),
        then: Then::Handover,
    };
    let challenge = Greeting::Challenge {
        nonce: "aa".to_owned(),
        proof: "bb".to_owned(),
    };
    let welcome = Greeting::Welcome {
        core: core.clone(),
        client_id: 7,
        scope: "full".to_owned(),
    };
    let refused = Greeting::Refused {
        code: Refusal::Version,
        reason: "update duskcode".to_owned(),
        core,
    };
    let core_json = r#"{"protocol":36,"oldest":36,"version":"0.77.0","owner":"Gui","busy":{"clients":2,"running":1}}"#;
    let pinned = [
        (
            serde_json::to_string(&hello).unwrap(),
            r#"{"protocol":36,"client":"bot","auth":{"Token":{"id":"q7m2k9xw3a"}},"nonce":"cc"}"#
                .to_owned(),
        ),
        (
            serde_json::to_string(&key).unwrap(),
            r#"{"protocol":36,"client":"tui","auth":"Key","nonce":""}"#.to_owned(),
        ),
        (
            serde_json::to_string(&proof).unwrap(),
            r#"{"proof":"dd","then":"Handover"}"#.to_owned(),
        ),
        (
            serde_json::to_string(&challenge).unwrap(),
            r#"{"Challenge":{"nonce":"aa","proof":"bb"}}"#.to_owned(),
        ),
        (
            serde_json::to_string(&welcome).unwrap(),
            format!(r#"{{"Welcome":{{"core":{core_json},"client_id":7,"scope":"full"}}}}"#),
        ),
        (
            serde_json::to_string(&refused).unwrap(),
            format!(
                r#"{{"Refused":{{"code":"Version","reason":"update duskcode","core":{core_json}}}}}"#
            ),
        ),
    ];
    for (actual, frozen) in pinned {
        assert_eq!(actual, frozen);
    }
}
