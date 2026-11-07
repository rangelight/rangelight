#![allow(clippy::unwrap_used, clippy::result_large_err)]

use secrecy::ExposeSecret;

use super::*;

#[derive(Debug, serde::Deserialize)]
struct Demo {
    url: String,
    port: u16,
    key: SecretRef,
}

const DEFAULTS: &str = "url = \"nats://default\"\nport = 1\nkey = \"secret://linear#api-key\"\n";

#[test]
fn defaults_then_file_then_env() {
    figment::Jail::expect_with(|jail| {
        let dir = jail.directory().to_owned();
        jail.create_file("demo.toml", "port = 2\nurl = \"nats://file\"")?;
        jail.set_env("RANGELIGHT_DEMO_URL", "nats://env");
        let demo: Demo = load_from("demo", DEFAULTS, &dir.join("demo.toml")).unwrap();
        assert_eq!((demo.url.as_str(), demo.port), ("nats://env", 2));
        assert_eq!(
            demo.key.path(),
            Path::new("/var/run/rangelight/secrets/linear/api-key")
        );
        let missing: Demo = load_from("demo", DEFAULTS, &dir.join("absent.toml")).unwrap();
        assert_eq!(missing.port, 1);
        Ok(())
    });
}

#[test]
fn a_bad_value_names_its_key() {
    figment::Jail::expect_with(|jail| {
        jail.set_env("RANGELIGHT_DEMO_PORT", "not-a-port");
        let err = load_from::<Demo>("demo", DEFAULTS, Path::new("/nonexistent")).unwrap_err();
        assert!(err.0.contains("port"), "{err}");
        Ok(())
    });
}

#[test]
fn secret_refs_cant_escape_their_secret() {
    for bad in [
        "secret://linear",
        "file:///etc/passwd",
        "secret://../x#k",
        "secret://a#..",
        "secret://a#b/c",
        "secret://#k",
    ] {
        assert!(SecretRef::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn read_strips_the_line_break_and_errors_name_only_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key");
    std::fs::write(&path, "s3cr3t-value\n").unwrap();
    assert_eq!(
        SecretRef::file(&path).read().unwrap().expose_secret(),
        "s3cr3t-value"
    );

    std::fs::write(&path, "\n").unwrap();
    assert!(matches!(
        SecretRef::file(&path).read(),
        Err(SecretError::Empty { .. })
    ));
    let missing = SecretRef::file(dir.path().join("absent"))
        .read()
        .unwrap_err();
    assert!(missing.to_string().contains("absent"));
}
