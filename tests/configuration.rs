use everledger::{ExitCode, config::Config};
use std::{
    collections::HashMap,
    io::Write,
    net::TcpListener,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[test]
fn invalid_daily_cap_precedes_every_argument_path() {
    let directory =
        std::env::temp_dir().join(format!("everledger-invalid-cap-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&finished);
    let server = std::thread::spawn(move || {
        let mut requests = 0;
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    requests += 1;
                    let _ = stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("fixture server: {e}"),
            }
        }
        requests
    });
    for args in [
        vec!["--help"],
        vec![],
        vec!["bogus"],
        vec!["status", "--json"],
        vec!["vendors", "2604", "--json"],
        vec!["refresh", "catalogue", "--json"],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_everledger"));
        command
            .args(&args)
            .env("EVERLEDGER_DATA_DIR", &directory)
            .env("EVERLEDGER_ROSTER_URL", &url)
            .env("EVERLEDGER_AHLEDGER_URL", &url)
            .env("EVERLEDGER_VENDOR_URL", &url)
            .env_remove("EVERLEDGER_FOREVERDB_API_KEY")
            .env_remove("EVERLEDGER_FOREVERDB_API_KEY_FILE")
            .env_remove("EVERLEDGER_FOREVERDB_VIA_CONNECTOR")
            .env("EVERLEDGER_FOREVERDB_DAILY_REQUESTS", "invalid");
        // Only a seam build can aim ForeverDB at the fixture. Any other build
        // runs without a ForeverDB credential, so a regression here cannot
        // reach the real service.
        #[cfg(feature = "parity-seams")]
        command
            .env("EVERLEDGER_SEAM_FOREVERDB_URL", &url)
            .env("EVERLEDGER_FOREVERDB_API_KEY", "fixture");
        let output = command.output().expect("run everledger");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(
            output.stderr, b"EVERLEDGER_FOREVERDB_DAILY_REQUESTS must be a positive integer\n",
            "{args:?}"
        );
    }
    finished.store(true, Ordering::Relaxed);
    assert_eq!(
        server.join().unwrap(),
        0,
        "invalid configuration sent a request"
    );
    assert_eq!(
        std::fs::read_dir(&directory).unwrap().count(),
        0,
        "invalid configuration wrote a file"
    );
    std::fs::remove_dir(&directory).unwrap();
}

#[test]
fn daily_cap_defaults_and_parses_python_integer_forms() {
    let default = Config::from_values(&HashMap::new()).unwrap();
    assert_eq!(default.foreverdb_daily_requests.get(), 36);
    for (value, expected) in [
        (" 40 ", 40),
        ("+40", 40),
        ("4_0", 40),
        ("0040", 40),
        ("1", 1),
        ("5000", 5000),
        ("4294967296", 4294967296),
        ("9223372036854775807", 9223372036854775807),
    ] {
        let mut env = HashMap::from([("EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(), value.into())]);
        let configured = Config::from_values(&env).unwrap();
        env.insert(
            "EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(),
            "invalid".into(),
        );
        assert_eq!(
            configured.foreverdb_daily_requests.get(),
            expected,
            "{value:?}"
        );
    }
}

#[test]
fn daily_cap_rejects_empty_nonpositive_and_noninteger_values() {
    for value in [
        "",
        " ",
        "invalid",
        "1.5",
        "0",
        "-0",
        "-1",
        "1e3",
        "0x24",
        "4 0",
        "_40",
        "٤٠",
        "9223372036854775808",
    ] {
        let error = Config::from_values(&HashMap::from([(
            "EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(),
            value.into(),
        )]))
        .unwrap_err();
        assert_eq!(error.code, ExitCode::Usage, "{value:?}");
        assert_eq!(
            error.message, "EVERLEDGER_FOREVERDB_DAILY_REQUESTS must be a positive integer",
            "{value:?}"
        );
    }
}
