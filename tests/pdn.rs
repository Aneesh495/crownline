use crownline::pdn::{Annotation, export, import};

#[test]
fn full_route_annotations_and_nested_variations_replay() {
    let text = r#"[GameType "21"]
[Event "branch \"study\""]
[Unknown "東京"]
{opening note} 1.9 -13! (1.10-14 {branch} 1...22-17 (1...21-17)) 1...21-17 *"#;
    let record = import(text, false).unwrap();
    assert_eq!(record.line.plies.len(), 2);
    let Annotation::Variation(branch) = &record.line.plies[0].annotations[1] else {
        panic!("missing branch")
    };
    assert_eq!(branch.plies[1].turn.to_string(), "22-17");
    assert_eq!(branch.plies[1].annotations.len(), 1);
    let canonical = export(&record);
    assert!(canonical.contains("東京"));
    assert_eq!(export(&import(&canonical, false).unwrap()), canonical);
}

#[test]
fn diagnostic_partial_and_ambiguous() {
    for (input, message) in [
        ("1.9-13", "truncated"),
        ("[Event \"x\"][Event \"y\"] *", "repeated"),
        ("[GameType \"21,B,8,8,A1,0\"] *", "unsupported"),
        ("1.9-13 2...21-17 *", "move number"),
        ("1.9-13 (1.10-14 *", "unexpected"),
        ("[FEN \"B:BK14:W10,11,18,19\"] 1.14x14 *", "ambiguous"),
    ] {
        assert!(
            import(input, false)
                .err()
                .unwrap()
                .message
                .contains(message),
            "{input}"
        );
    }
    let partial = import("1.9-13 21-17 2. {unterminated", true).unwrap();
    assert_eq!(partial.line.plies.len(), 2);
    assert!(partial.diagnostic.is_some());
    assert_eq!(partial.result, "*");
}
