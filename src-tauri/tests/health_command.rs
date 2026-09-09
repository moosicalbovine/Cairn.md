use notemd_lib::{health_payload, HealthStatus};

#[test]
fn health_payload_contains_only_public_application_state() {
    let status = health_payload();

    assert_eq!(status.app, "NoteMD");
    assert_eq!(status.status, HealthStatus::Ok);
    assert!(!status.version.is_empty());
}
