use std::path::PathBuf;

use file_core::FileKind;

use crate::profile::{ContentIndexPolicy, IndexProfile, MediaMetadataPolicy, MediaMetadataScope};
use crate::search::{
    DirectoryErrorPolicy, FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexStatus,
    FileSearchMatch, FileSearchOutcome, MediaExifField, MediaSearchKind, MediaSearchMetadata,
    SearchResultSource,
};
use crate::service::{IndexServiceCommand, IndexServiceEvent};

use super::{IndexRequest, IndexRequestCommand, IndexResponse, WirePath};

#[test]
fn protocol_version_changes_when_wire_schema_changes() {
    assert_eq!(super::INDEX_PROTOCOL_VERSION, 3);
}

#[test]
fn new_command_variants_are_appended_after_version_one_commands() {
    assert_eq!(
        command_discriminant(&IndexRequestCommand::LoadProfile("main".to_owned())),
        1
    );
    assert_eq!(
        command_discriminant(&IndexRequestCommand::Status {
            profile_id: "main".to_owned(),
            root: WirePath::from_path(PathBuf::from("/tmp/root").as_path()),
        }),
        5
    );
    assert_eq!(command_discriminant(&IndexRequestCommand::Ping), 12);
    assert_eq!(
        command_discriminant(&IndexRequestCommand::StartMaintenance {
            profile_id: "main".to_owned(),
        }),
        13
    );
    assert_eq!(command_discriminant(&IndexRequestCommand::Shutdown), 14);
}

#[test]
fn shutdown_command_round_trip_preserves_variant() {
    let request = IndexRequest::from_command("/cache/index", IndexServiceCommand::Shutdown);
    let restored = request.command.into_service_command().unwrap();

    assert_eq!(restored, IndexServiceCommand::Shutdown);
}

#[test]
fn command_round_trip_preserves_profile_paths() {
    let command = IndexServiceCommand::ConfigureProfile(IndexProfile {
        id: "main".to_owned(),
        roots: vec![PathBuf::from("/tmp/root"), PathBuf::from("/tmp/root/src")],
        include_hidden: true,
        exclude_patterns: vec!["target/".to_owned()],
        directory_error_policy: DirectoryErrorPolicy::Abort,
        content: ContentIndexPolicy {
            enabled: true,
            max_file_bytes: 4096,
        },
        media: MediaMetadataPolicy {
            scope: MediaMetadataScope::All,
        },
    });

    let request = IndexRequest::from_command("/cache/index", command.clone());
    let restored = request.command.into_service_command().unwrap();

    assert_eq!(restored, command);
}

#[test]
fn selected_path_command_round_trip_preserves_paths() {
    let command =
        IndexServiceCommand::BuildSelectedPaths(crate::service::BuildSelectedPathsRequest {
            profile_id: "main".to_owned(),
            root: PathBuf::from("/tmp/root"),
            selected_paths: vec![PathBuf::from("/tmp/root/src")],
            mode: FileSearchIndexMode::Incremental,
        });

    let request = IndexRequest::from_command("/cache/index", command.clone());
    let restored = request.command.into_service_command().unwrap();

    assert_eq!(restored, command);
}

#[test]
fn protocol_mismatch_response_round_trips() {
    let response = IndexResponse::ProtocolMismatch {
        expected: super::INDEX_PROTOCOL_VERSION,
        actual: super::INDEX_PROTOCOL_VERSION + 1,
    };

    let encoded = bincode::serialize(&response).unwrap();
    let decoded: IndexResponse = bincode::deserialize(&encoded).unwrap();

    assert_eq!(decoded, response);
}

#[test]
fn query_response_round_trip_preserves_media_metadata() {
    let event = IndexServiceEvent::QueryFinished(FileSearchOutcome {
        root: PathBuf::from("/tmp/root"),
        matches: vec![FileSearchMatch {
            path: PathBuf::from("/tmp/root/photo.jpg"),
            relative_path: PathBuf::from("photo.jpg"),
            name: "photo.jpg".into(),
            kind: FileKind::File,
            rank_score: 42,
            source: SearchResultSource::Media,
            snippet: Some("camera".to_owned()),
            media: Some(MediaSearchMetadata {
                media_kind: MediaSearchKind::Image,
                width: Some(320),
                height: Some(240),
                duration_ms: None,
                codec: Some("jpeg".to_owned()),
                title: Some("Trip".to_owned()),
                artist: Some("User".to_owned()),
                exif: vec![MediaExifField {
                    tag: "Make".to_owned(),
                    value: "Camera".to_owned(),
                }],
            }),
        }],
        skipped: Vec::new(),
    });

    assert_response_event_round_trips(event);
}

#[test]
fn status_response_round_trip_preserves_readiness_and_failures() {
    let event = IndexServiceEvent::StatusLoaded(FileSearchIndexStatus {
        root: PathBuf::from("/tmp/root"),
        index_dir: PathBuf::from("/tmp/index"),
        exists: true,
        stale: true,
        reason: Some("search index media policy is outdated".to_owned()),
        include_hidden: true,
        content_index_enabled: true,
        content_max_file_bytes: 4096,
        media_metadata_scope: MediaMetadataScope::All,
        record_count: 7,
        index_size_bytes: 2048,
        built_at_ms: Some(11),
        updated_at_ms: Some(22),
        failed_count: 1,
        exclude_rules_hash: Some("hash".to_owned()),
        extractor_version: Some(3),
        failures: vec![FileSearchIndexFailure {
            path: PathBuf::from("/tmp/root/private"),
            message: "permission denied".to_owned(),
            first_failed_at_ms: 1,
            last_failed_at_ms: 2,
            retry_count: 3,
        }],
    });

    assert_response_event_round_trips(event);
}

#[test]
fn pong_response_round_trip_preserves_daemon_version() {
    assert_response_event_round_trips(IndexServiceEvent::Pong {
        daemon_version: "1.2.3".to_owned(),
    });
}

#[test]
fn shutdown_response_round_trips() {
    assert_response_event_round_trips(IndexServiceEvent::Shutdown);
}

#[test]
fn request_can_represent_mismatched_protocol_version() {
    let request = IndexRequest {
        version: super::INDEX_PROTOCOL_VERSION + 1,
        index_base_dir: WirePath::from_path(PathBuf::from("/tmp/index-base").as_path()),
        command: IndexRequestCommand::LoadProfile("main".to_owned()),
    };

    assert_eq!(request.version, super::INDEX_PROTOCOL_VERSION + 1);
}

fn assert_response_event_round_trips(event: IndexServiceEvent) {
    let response = IndexResponse::from_event(&event);
    let encoded = bincode::serialize(&response).unwrap();
    let decoded: IndexResponse = bincode::deserialize(&encoded).unwrap();
    let IndexResponse::Event(decoded_event) = decoded else {
        panic!("expected event response");
    };

    assert_eq!(decoded_event.into_domain(), event);
}

fn command_discriminant(command: &IndexRequestCommand) -> u32 {
    let encoded = bincode::serialize(command).unwrap();
    u32::from_le_bytes(encoded[0..4].try_into().unwrap())
}
