#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::FolderV1;
use reopen_core::{
    AdapterRef, CaptureReason, CheckpointId, Completeness, Resource, ResourceId, ResourcePayload,
    SourceState, SourceStatus, Timestamp,
};
use reopen_storage::{CommitOutcome, NewCheckpoint, PinReason, Store, StoreError};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

#[test]
fn history_status_and_detail_round_trip() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(
        &dir.path().join("reopen.sqlite"),
        at("2026-10-23T10:00:00Z"),
    )
    .unwrap();
    let workspace = store
        .ensure_default_workspace(at("2026-10-23T10:00:00Z"))
        .unwrap();
    let session = store
        .start_session(at("2026-10-23T10:00:00Z"))
        .unwrap()
        .session_id;
    assert_eq!(store.latest_checkpoint().unwrap(), None);

    let mut ids = Vec::new();
    for (minute, completeness_source) in [(1, SourceState::Complete), (2, SourceState::Unavailable)]
    {
        let when = at(&format!("2026-10-23T10:0{minute}:00Z"));
        let source = SourceStatus {
            source_key: "desktop.apps".to_owned(),
            state: completeness_source,
            required: true,
            captured_at: (completeness_source == SourceState::Complete).then_some(when),
            last_seen_at: None,
            error: (completeness_source != SourceState::Complete).then_some(ProviderError::Timeout),
        };
        let resource = Resource {
            id: ResourceId::new(),
            adapter: AdapterRef {
                id: "fake.folders".to_owned(),
                version: 1,
            },
            ordinal: 0,
            payload: ResourcePayload::FolderV1(FolderV1 {
                path: format!("/home/u/{minute}"),
            }),
        };
        let outcome = store
            .commit_checkpoint(&NewCheckpoint {
                workspace_id: workspace,
                session_id: session,
                captured_at: when,
                reason: CaptureReason::Scheduled,
                sources: vec![source],
                resources: vec![resource],
            })
            .unwrap();
        let CommitOutcome::Created { id, .. } = outcome else {
            panic!()
        };
        ids.push(id);
    }
    store
        .pin_checkpoint(ids[0], PinReason::User, "", at("2026-10-23T10:05:00Z"))
        .unwrap();

    let history = store.list_checkpoints(10).unwrap();
    assert_eq!(
        history.iter().map(|c| c.id).collect::<Vec<_>>(),
        [ids[1], ids[0]]
    );
    assert_eq!(history[0].completeness, Completeness::Partial);
    assert!(history[1].protected && !history[0].protected);
    assert_eq!(history[0].resource_count, 1);
    assert_eq!(store.list_checkpoints(1).unwrap().len(), 1);
    assert_eq!(store.latest_checkpoint().unwrap().unwrap().id, ids[1]);
    assert_eq!(store.pending_candidate_count().unwrap(), 0);

    let detail = store.checkpoint_detail(ids[1]).unwrap();
    assert_eq!(detail.sources[0].error, Some(ProviderError::Timeout));
    assert_eq!(detail.sources[0].captured_at, None);
    assert_eq!(
        detail.resources[0].payload,
        ResourcePayload::FolderV1(FolderV1 {
            path: "/home/u/2".to_owned()
        })
    );
    assert!(matches!(
        store.checkpoint_detail(CheckpointId::new()),
        Err(StoreError::NotFound)
    ));
}
