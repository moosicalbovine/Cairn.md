use cairn_md_lib::{health_payload, HealthStatus};

#[test]
fn health_payload_contains_only_public_application_state() {
    let status = health_payload();

    assert_eq!(status.app, "Cairn.md");
    assert_eq!(status.status, HealthStatus::Ok);
    assert!(!status.version.is_empty());
}
