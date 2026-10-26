//! Offline regression for the AWS signing capabilities enabled by our S3 feature.
//! No AWS account, network request, or real credentials are used.
#![cfg(feature = "s3")]

use aws_sdk_s3::{
    config::{BehaviorVersion, Credentials, Region},
    presigning::PresigningConfig,
    Client,
};
use std::time::Duration;

#[tokio::test]
async fn sdk_presigns_multi_region_access_points_and_regional_buckets() {
    let client = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(Credentials::new(
                "AKIDEXAMPLE",
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                Some("synthetic-session-token".into()),
                None,
                "offline-test",
            ))
            .build(),
    );
    let expires = || PresigningConfig::expires_in(Duration::from_secs(300)).unwrap();
    let mrap = client
        .get_object()
        .bucket("arn:aws:s3::123456789012:accesspoint/example.mrap")
        .key("tenant/document.pdf")
        .presigned(expires())
        .await
        .expect("SigV4a must remain available");
    let uri = mrap.uri();
    assert!(uri.starts_with("https://example.mrap.accesspoint.s3-global.amazonaws.com/"));
    assert!(uri.contains("X-Amz-Algorithm=AWS4-ECDSA-P256-SHA256"));
    assert!(uri.contains("X-Amz-Region-Set=%2A") || uri.contains("X-Amz-Region-Set=*"));
    assert!(uri.contains("X-Amz-Security-Token=synthetic-session-token"));
    assert!(uri.contains("X-Amz-Signature="));

    let regional = client
        .get_object()
        .bucket("example-bucket")
        .key("tenant/document.pdf")
        .presigned(expires())
        .await
        .expect("regional SigV4 signing must remain available");
    assert!(regional.uri().contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
    assert!(regional.uri().contains("X-Amz-Signature="));
}
