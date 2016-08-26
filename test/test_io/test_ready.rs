use tokio::io::Ready;

#[test]
fn test_no_readiness() {
    let ready = Ready::none();
    assert!(!ready.is_readable());
    assert!(!ready.is_writable());
}

#[test]
fn test_read_rediness() {
    let ready = Ready::readable();
    assert!(ready.is_readable());
    assert!(!ready.is_writable());
}

#[test]
fn test_write_rediness() {
    let ready = Ready::writable();
    assert!(!ready.is_readable());
    assert!(ready.is_writable());
}

#[test]
fn test_read_and_write_readiness() {
    let ready = Ready::all();
    assert!(ready.is_readable());
    assert!(ready.is_writable());
}

#[test]
fn test_contains_readiness() {
    assert!(!Ready::none().contains(Ready::readable()));
    assert!(!Ready::none().contains(Ready::writable()));

    assert!(Ready::all().contains(Ready::readable()));
    assert!(Ready::all().contains(Ready::writable()));

    assert!(Ready::readable().contains(Ready::readable()));
    assert!(!Ready::readable().contains(Ready::writable()));

    assert!(Ready::writable().contains(Ready::writable()));
    assert!(!Ready::writable().contains(Ready::readable()));
}

#[test]
fn test_ready_bitwise_ops() {
    assert_eq!(Ready::all(), Ready::readable() | Ready::writable());
    assert_eq!(Ready::none(), Ready::readable() & Ready::writable());
    assert_eq!(Ready::all(), Ready::readable() ^ Ready::writable());
    assert_eq!(Ready::none(), Ready::readable() ^ Ready::readable());
}
