#![allow(clippy::unwrap_used)]

use super::*;

const SCHEMA: &str = include_str!("../../../../../api/jsonschema/terminal.v1alpha1.schema.json");

/// The frame rule's limits are the schema's.
#[test]
fn the_limits_are_the_schema_s() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let defs = &schema["$defs"];
    let n = |v: &serde_json::Value| usize::try_from(v.as_u64().unwrap()).unwrap();
    assert_eq!(n(&defs["bytes"]["minLength"]), DATA_MIN);
    assert_eq!(n(&defs["bytes"]["maxLength"]), DATA_MAX);
    for size in ["cols", "rows"] {
        let cells = &defs["resize"]["properties"][size];
        assert_eq!(n(&cells["minimum"]), usize::from(*CELLS.start()));
        assert_eq!(n(&cells["maximum"]), usize::from(*CELLS.end()));
    }
}

/// The schema's examples pass or fail here as they do against the schema.
#[test]
fn the_schema_s_examples_hold() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../api/examples");
    for (dir, valid) in [
        (examples.to_owned(), true),
        (format!("{examples}/invalid"), false),
    ] {
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if !path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("terminal.")
            {
                continue;
            }
            let text = std::fs::read(&path).unwrap();
            assert_eq!(Frame::parse(&text).is_some(), valid, "{}", path.display());
            seen += 1;
        }
        assert!(seen > 0, "no terminal examples in {dir}");
    }
}

#[test]
fn frames_hold_to_the_schema() {
    let max = "A".repeat(DATA_MAX);
    let over = "A".repeat(DATA_MAX + 4);
    let ok = [
        r#"{"type":"input","data":"bHM="}"#.to_owned(),
        r#"{"data":"bA==","type":"output"}"#.to_owned(),
        r#"{"type":"resize","cols":1000,"rows":1}"#.to_owned(),
        format!(r#"{{"type":"input","data":"{max}"}}"#),
    ];
    let bad = [
        r#"{"type":"input","data":"bHM"}"#.to_owned(),
        r#"{"type":"input","data":"b=M="}"#.to_owned(),
        r#"{"type":"input","data":"bHM=","x":1}"#.to_owned(),
        r#"{"type":"input"}"#.to_owned(),
        r#"{"type":"paste","data":"bHM="}"#.to_owned(),
        r#"{"type":"resize","cols":1001,"rows":1}"#.to_owned(),
        r#"{"type":"resize","cols":-1,"rows":1}"#.to_owned(),
        r#"{"type":"resize","cols":"80","rows":24}"#.to_owned(),
        "not json".to_owned(),
        format!(r#"{{"type":"input","data":"{over}"}}"#),
    ];
    for frame in ok {
        assert!(Frame::parse(frame.as_bytes()).is_some(), "{frame:.60}");
    }
    for frame in bad {
        assert!(Frame::parse(frame.as_bytes()).is_none(), "{frame:.60}");
    }
}
