//! The built helper started dry: it speaks the whole protocol and injects nothing, watches
//! nothing. These prove the protocol, the grant rules and the way the helper ends, on any machine.

mod common;

use std::thread;
use std::time::Duration;

use common::{DRY_DISPLAY, Helper, grant, kind};
use input_helper::json::{self, Value};

/// Keywork-app's `fixtures/screen/consent-rules.json`, the table its grant book and its stand-in
/// helper also run, copied byte for byte.
const CONSENT_RULES: &str = include_str!("consent-rules.json");
const DRY_HELLO: &str = concat!(
    r#"{"v":1,"kind":"hello","version":1,"impl":"kw-input-helper "#,
    env!("CARGO_PKG_VERSION"),
    r#" dry-run"}"#
);

fn greeted() -> Helper {
    let mut helper = Helper::dry();
    assert_eq!(helper.greet(), DRY_HELLO);
    helper
}

#[test]
fn the_shared_consent_rules_hold() {
    let rules = json::parse(CONSENT_RULES).expect("the table is JSON");
    let Some(Value::List(cases)) = rules.field("cases") else { panic!("the table has cases") };
    assert!(!cases.is_empty());
    for case in cases {
        let Some(Value::Text(name)) = case.field("name") else { panic!("a case has a name") };
        let Some(Value::List(steps)) = case.field("steps") else { panic!("{name}: steps") };
        let mut helper = greeted();
        let mut seq = 0;
        for step in steps {
            let number = |field| match step.field(field) {
                Some(Value::Number(number)) => *number,
                _ => panic!("{name}: a step's {field}"),
            };
            match step.field("do") {
                Some(Value::Text(action)) if action == "grant" => {
                    helper.send(&grant(number("lifetimeMs"), DRY_DISPLAY));
                    let replies = helper.replies_until(|reply| kind(reply).is_some());
                    assert!(
                        replies.last().is_some_and(|line| line.contains(r#""kind":"granted""#)),
                        "{name}"
                    );
                }
                Some(Value::Text(action)) if action == "wait" => {
                    thread::sleep(Duration::from_millis(number("ms") as u64));
                }
                Some(Value::Text(action)) if action == "restart" => {
                    helper.stop();
                    helper = greeted();
                }
                Some(Value::Text(action)) if action == "input" => {
                    seq += 1;
                    helper.send(&format!(
                        r#"{{"v":1,"kind":"key","grantId":"g1","seq":{seq},"code":"KeyA","down":false}}"#
                    ));
                    let answered = |reply: &Value| {
                        kind(reply) == Some("bye")
                            || reply.field("seq") == Some(&Value::Number(f64::from(seq)))
                    };
                    let replies = helper.replies_until(answered);
                    let last = replies.last().and_then(|line| json::parse(line));
                    let answer =
                        if last.as_ref().and_then(kind) == Some("applied") { "applied" } else { "refused" };
                    let Some(Value::Text(expected)) = step.field("expect") else { panic!("{name}: expect") };
                    assert_eq!(answer, expected, "{name}, input {seq}");
                }
                _ => panic!("{name}: a step it does not know"),
            }
        }
    }
}

#[test]
fn lets_go_of_everything_it_holds_and_leaves_when_its_input_closes() {
    let mut helper = greeted();
    helper.send(&grant(60_000.0, DRY_DISPLAY));
    helper.send(r#"{"v":1,"kind":"key","grantId":"g1","seq":1,"code":"KeyQ","down":true}"#);
    helper.send(
        r#"{"v":1,"kind":"ptr","grantId":"g1","seq":1,"action":"down","x":0.5,"y":0.5,"button":"left"}"#,
    );
    let replies = helper.replies_until(|reply| kind(reply) == Some("applied"));
    assert!(replies[0].contains(r#""kind":"granted""#));
    assert_eq!(helper.reply().as_deref(), Some(r#"{"v":1,"kind":"applied","seq":1}"#));
    helper.close_input();
    assert_eq!(
        helper.reply().as_deref(),
        Some(r#"{"v":1,"kind":"released","keys":["KeyQ"],"buttons":["left"]}"#)
    );
    assert_eq!(helper.reply().as_deref(), Some(r#"{"v":1,"kind":"bye"}"#));
    assert_eq!(helper.exit_code(), Some(0));
}

#[test]
fn refuses_a_version_it_does_not_speak_and_leaves() {
    let mut helper = Helper::dry();
    helper.send(r#"{"v":1,"kind":"hello","versions":[99]}"#);
    let replies: Vec<String> = (0..3).filter_map(|_| helper.reply()).collect();
    assert_eq!(
        replies,
        [
            r#"{"v":1,"kind":"refused","reason":"version"}"#,
            r#"{"v":1,"kind":"released","keys":[],"buttons":[]}"#,
            r#"{"v":1,"kind":"bye"}"#
        ]
    );
    assert_eq!(helper.exit_code(), Some(2));
}
