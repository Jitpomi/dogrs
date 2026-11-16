#![cfg(feature = "iroh")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Payload {
    tenant: String,
    sequence: u64,
    data: Vec<u8>,
    optional: Option<String>,
}
#[derive(Deserialize)]
struct Fixture {
    sequence: u64,
    size: usize,
    hash: String,
    postcard: Vec<u8>,
}

#[tokio::test]
async fn previous_release_store_and_postcard_bytes_remain_readable() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let archive = include_bytes!("fixtures/iroh-0103-postcard-113.tar.gz");
    tar::Archive::new(flate2::read::GzDecoder::new(&archive[..])).unpack(temporary.path())?;
    let fixtures: Vec<Fixture> = serde_json::from_slice(&std::fs::read(
        temporary.path().join("legacy-formats.json"),
    )?)?;
    let path = temporary.path().join("legacy-store");
    let store = iroh_blobs::store::fs::FsStore::load(&path).await?;
    for fixture in fixtures {
        let data: Vec<u8> = (0..fixture.size).map(|n| (n % 251) as u8).collect();
        let hash: iroh_blobs::Hash = fixture.hash.parse()?;
        assert_eq!(store.get_bytes(hash).await?.as_ref(), data.as_slice());
        let decoded: Payload = postcard::from_bytes(&fixture.postcard)?;
        let expected = Payload {
            tenant: "synthetic-tenant".into(),
            sequence: fixture.sequence,
            data,
            optional: Some("preserved".into()),
        };
        assert_eq!(decoded, expected);
        assert_eq!(postcard::to_stdvec(&expected)?, fixture.postcard);
        assert_eq!(
            postcard::to_vec::<_, 262144>(&expected)?.as_slice(),
            fixture.postcard.as_slice()
        );
    }
    let current = store
        .add_bytes(b"current release after compatibility check".to_vec())
        .await?;
    store.tags().set("current-fixture", current.hash).await?;
    store.shutdown().await?;
    drop(store);
    let reopened = iroh_blobs::store::fs::FsStore::load(&path).await?;
    assert_eq!(
        reopened.get_bytes(current.hash).await?.as_ref(),
        b"current release after compatibility check"
    );
    reopened.shutdown().await?;
    Ok(())
}
