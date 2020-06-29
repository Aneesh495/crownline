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

#[test]
fn archive_boundaries_follow_tokens_and_keep_comment_text() {
    let archive = r#"[Event "game [Event inside text]"] {fake result 1-0}
1.9-13 (1.10-14) 21-17 1/2-1/2 {trailing [Event "fake"]}
[Event "second"] 1.11-15 22-18 1-1
"#;
    let spans = crownline::pdn::archive_ranges(archive).unwrap();
    assert_eq!(spans.len(), 2);
    let first = import(&archive[spans[0].clone()], false).unwrap();
    let second = import(&archive[spans[1].clone()], false).unwrap();
    assert_eq!(first.line.plies.len(), 2);
    assert_eq!(second.result, "1-1");
    assert!(export(&first).contains("trailing [Event"));
    assert!(crownline::pdn::archive_ranges("   ").is_err());
    assert!(crownline::pdn::archive_ranges("1.9-13 ) *").is_err());
    assert!(import(&archive[spans[0].start..spans[1].end], false).is_err());
}

#[test]
fn legacy_dash_captures_require_the_declared_dialect() {
    let body = "[FEN \"B:BK14:W10,11,18,19\"] 1.14-7-16-23-14 1-0";
    assert!(import(body, false).is_err());
    let text = format!("[CaptureNotation \"LegacyDash\"][ResultOrder \"BlackFirst\"]{body}");
    let record = import(&text, false).unwrap();
    assert_eq!(record.line.plies[0].turn.to_string(), "14x7x16x23x14");
    assert_eq!(
        export(&import(&export(&record), false).unwrap()),
        export(&record)
    );
    let ambiguous = "[CaptureNotation \"LegacyDash\"][FEN \"B:BK14:W10,11,18,19\"] 14-14 *";
    assert!(
        import(ambiguous, false)
            .err()
            .unwrap()
            .message
            .contains("ambiguous")
    );
}

#[test]
fn international_result_codes_check_terminal_winner() {
    for (order, result) in [("BlackFirst", "2-0"), ("WhiteFirst", "0-2")] {
        let text = format!("[ResultOrder \"{order}\"][FEN \"B:BK14:W18\"] 14x23 {result}");
        let record = import(&text, false).unwrap();
        assert_eq!(record.result, result);
        assert_eq!(import(&export(&record), false).unwrap().result, result);
    }
    let draw = "[ResultOrder \"BlackFirst\"][FEN \"B:BK14:W18\"] 14x23 1-1";
    assert!(
        import(draw, false)
            .err()
            .unwrap()
            .message
            .contains("result conflicts")
    );
}
