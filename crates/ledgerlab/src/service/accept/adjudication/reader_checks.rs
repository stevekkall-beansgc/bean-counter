//! Adapter conformance against the actual committed customer witness.
use super::*;
use crate::store::adjudication::{
    AdjudicationReadStore, AdjudicationReadTx, IndexedPageRequest, ObjectPageRequest,
    SnapshotSelection,
};
use crate::store::ports::{AcceptanceStore, AcceptanceTx};
use r3::reads::PageAddress;

fn budget() -> wire::ReadBudget {
    wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).unwrap(),
        pages: Count::new(4096).unwrap(),
        segments: Count::new(4096).unwrap(),
    }
}
async fn segment<T: AdjudicationReadTx>(read: &mut T, hash: &Digest) -> wire::Segment {
    let mut bytes = Vec::new();
    loop {
        let q = IndexedPageRequest {
            address: PageAddress {
                segment: hash.clone(),
                page: Count::new((bytes.len() / 4096) as u128).unwrap(),
            },
            offset: 0,
            max_bytes: 4096,
        };
        let page = read.segment_page(&q).await.unwrap();
        bytes.extend(page.bytes);
        if bytes.len() as u128 == page.total_bytes.value() {
            break;
        }
    }
    let segment: wire::Segment = r3::parse_exact(&bytes, r3::SEGMENT_BYTES).unwrap();
    assert_eq!(&rt::hash("segment", &segment).unwrap(), hash);
    segment
}
pub(super) async fn check(store: &SqliteStore, witness: &[Value]) {
    store.test_reader_indices().await;
    let before = store.test_full_inventory().await;
    let selection = SnapshotSelection {
        journal: journal("center"),
        historical: None,
    };
    let mut read = store
        .begin_adjudication_read(&selection, &budget(), deadline())
        .await
        .unwrap();
    let current = read.expected_prefix().expected().clone();
    assert_eq!(read.lease().retained_bytes, Count::ZERO);
    let mut hash = current.segment.clone();
    let mut prefixes = Vec::new();
    let central: Vec<_> = witness.iter().filter(|s| s["host"] == "center").collect();
    assert_eq!(current.ordinal.value(), central.len() as u128);
    for step in central.iter().rev() {
        let s = segment(&mut read, &hash).await;
        assert_eq!(serde_json::to_value(&s.command).unwrap(), step["command"]);
        assert_eq!(serde_json::to_value(&s.result).unwrap(), step["result"]);
        let mut prefix = current.clone();
        prefix.ordinal = s.ordinal;
        prefix.segment = hash;
        prefix.root = s.result.root.clone();
        prefixes.push(prefix);
        // Read exact full source identity, including retained cross-host origins.
        for object in &s.objects {
            let mut bytes = Vec::new();
            while (bytes.len() as u128) < object.bytes.value() {
                bytes.extend(
                    read.object_page(&ObjectPageRequest {
                        origin: object.origin.clone(),
                        kind: object.kind.clone(),
                        key: r3::canonical_bytes(&object.full_key, 4096).unwrap(),
                        hash: object.body_hash.clone(),
                        offset: Count::new(bytes.len() as u128).unwrap(),
                        max_bytes: 4096,
                    })
                    .await
                    .unwrap(),
                );
            }
            assert_eq!(
                bytes,
                r3::proofs::decode_base64(&object.body, 262144).unwrap()
            );
        }
        hash = s.previous;
    }
    let close = central
        .iter()
        .find(|s| s["command"]["kind"] == "CLOSE")
        .unwrap();
    let cert: wire::Certificate =
        serde_json::from_value(close["result"]["effects"][0]["body"].clone()).unwrap();
    for family in &cert.unavailable {
        assert_eq!(
            read.family_certificate(family, &current).await.unwrap(),
            Some(cert.clone())
        );
    }
    read.finish().await.unwrap();
    let initial = prefixes.last().unwrap().clone();
    let historical = SnapshotSelection {
        journal: journal("center"),
        historical: Some(initial.clone()),
    };
    let mut read = store
        .begin_adjudication_read(&historical, &budget(), deadline())
        .await
        .unwrap();
    assert_eq!(read.expected_prefix().expected(), &initial);
    assert!(read
        .family_certificate(&cert.unavailable[0], &initial)
        .await
        .unwrap()
        .is_none());
    assert!(read
        .segment_page(&IndexedPageRequest {
            address: PageAddress {
                segment: current.segment.clone(),
                page: Count::ZERO
            },
            offset: 0,
            max_bytes: 4096
        })
        .await
        .is_err());
    drop(read);
    let mut wrong = historical.clone();
    wrong.historical.as_mut().unwrap().root = current.root.clone();
    assert!(store
        .begin_adjudication_read(&wrong, &budget(), deadline())
        .await
        .is_err());
    let mut tiny = budget();
    tiny.bytes = Count::new(1).unwrap();
    assert!(store
        .begin_adjudication_read(&selection, &tiny, deadline())
        .await
        .is_err());
    // An idle client cannot keep the real snapshot/write barrier beyond its lease.
    let expiry = Instant::now() + Duration::from_millis(200);
    let mut idle = store
        .begin_adjudication_read(&selection, &budget(), expiry)
        .await
        .unwrap();
    assert!(store
        .begin(Instant::now() + Duration::from_millis(30))
        .await
        .is_err());
    tokio::time::sleep_until(expiry + Duration::from_millis(50)).await;
    assert!(idle
        .segment_page(&IndexedPageRequest {
            address: PageAddress {
                segment: current.segment,
                page: Count::ZERO
            },
            offset: 0,
            max_bytes: 4096
        })
        .await
        .is_err());
    drop(idle);
    store
        .begin(deadline())
        .await
        .unwrap()
        .rollback()
        .await
        .unwrap();
    let read = store
        .begin_adjudication_read(&selection, &budget(), deadline())
        .await
        .unwrap();
    let other = store.clone();
    let other_selection = selection.clone();
    let queued = tokio::spawn(async move {
        other
            .begin_adjudication_read(&other_selection, &budget(), deadline())
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    queued.abort();
    assert!(matches!(queued.await, Err(e) if e.is_cancelled()));
    // Drop both the active client and a caller canceled before initialization.
    // The owned workers must release their real snapshots before this writer.
    drop(read);
    store
        .begin(deadline())
        .await
        .unwrap()
        .rollback()
        .await
        .unwrap();
    assert_eq!(store.test_full_inventory().await, before);
    eprintln!("actual reader: {} central segments and exact objects, historical suffix refusal, all closure certificates, budget and idle expiry PASS",central.len());
}
