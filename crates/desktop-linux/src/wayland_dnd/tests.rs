use super::payload::negotiated_drop_action;
use super::*;
use crate::file_clipboard::FileClipboardOperation;

#[test]
fn drag_payload_uses_move_operation_and_uri_list_line_endings() {
    let paths = vec![PathBuf::from("/tmp/a b"), PathBuf::from("/tmp/c")];

    let payload = DragPayload::new(&paths);

    assert_eq!(
        payload.for_mime(URI_LIST_MIME),
        Some("file:///tmp/a%20b\r\nfile:///tmp/c\r\n")
    );
    assert_eq!(
        payload.for_mime(GNOME_COPIED_FILES_MIME),
        Some("cut\nfile:///tmp/a%20b\nfile:///tmp/c")
    );
}

#[test]
fn drop_payload_forces_copy_even_if_gnome_payload_says_cut() {
    let selection =
        parse_drop_selection(GNOME_COPIED_FILES_MIME, b"cut\nfile:///tmp/source").unwrap();

    assert_eq!(selection.selection.operation, FileClipboardOperation::Copy);
    assert_eq!(
        selection.selection.paths,
        vec![PathBuf::from("/tmp/source")]
    );
    assert_eq!(
        drop_origin_for_mime(GNOME_COPIED_FILES_MIME, None).unwrap(),
        WaylandDndDropOrigin::External
    );
}

#[test]
fn internal_mime_negotiates_move_while_external_mimes_negotiate_copy() {
    assert_eq!(
        negotiated_drop_action(INTERNAL_FILE_DRAG_MIME),
        DndAction::Move
    );
    assert_eq!(
        negotiated_drop_action(GNOME_COPIED_FILES_MIME),
        DndAction::Copy
    );
    assert_eq!(negotiated_drop_action(URI_LIST_MIME), DndAction::Copy);
}

#[test]
fn internal_drag_payload_requires_and_preserves_source_session() {
    let controller = WaylandDndController::new();
    let session_id = controller
        .start_file_drag(vec![PathBuf::from("/tmp/source")], test_drag_icon())
        .unwrap();
    let selection =
        parse_drop_selection(INTERNAL_FILE_DRAG_MIME, b"file:///tmp/source\r\n").unwrap();

    assert_eq!(selection.selection.operation, FileClipboardOperation::Copy);
    assert_eq!(
        selection.selection.paths,
        vec![PathBuf::from("/tmp/source")]
    );
    assert_eq!(
        drop_origin_for_mime(INTERNAL_FILE_DRAG_MIME, Some(session_id)).unwrap(),
        WaylandDndDropOrigin::Internal(session_id)
    );
    assert!(matches!(
        drop_origin_for_mime(INTERNAL_FILE_DRAG_MIME, None),
        Err(WaylandDndError::InternalDropSessionUnavailable)
    ));
}

fn test_drag_icon() -> WaylandFileDragIcon {
    WaylandFileDragIcon::new(1, 1, vec![10, 20, 30, 255]).unwrap()
}

#[test]
fn controller_sends_identified_file_drag_command_to_worker_receiver() {
    let controller = WaylandDndController::new();
    let path = PathBuf::from("/tmp/source");

    let session_id = controller
        .start_file_drag(vec![path.clone()], test_drag_icon())
        .unwrap();

    let mut command_receiver = controller.take_command_receiver().unwrap();
    let command = command_receiver.try_recv().unwrap();
    match command {
        WaylandDndCommand::StartFileDrag {
            session_id: received_session_id,
            paths,
            icon,
        } => {
            assert_eq!(received_session_id, session_id);
            assert_eq!(paths, vec![path]);
            assert_eq!(icon.width(), 1);
            assert_eq!(icon.height(), 1);
            assert_eq!(icon.premultiplied_rgba(), &[10, 20, 30, 255]);
        }
    }
}

#[test]
fn self_target_events_preserve_source_session_identity() {
    let controller = WaylandDndController::new();
    let session_id = controller
        .start_file_drag(vec![PathBuf::from("/tmp/source")], test_drag_icon())
        .unwrap();

    for event in [
        WaylandFileDragSelfTargetEvent::Entered {
            session_id,
            position: WaylandDndDropPosition { x: 1.0, y: 2.0 },
        },
        WaylandFileDragSelfTargetEvent::Moved {
            session_id,
            position: WaylandDndDropPosition { x: 3.0, y: 4.0 },
        },
        WaylandFileDragSelfTargetEvent::Left { session_id },
    ] {
        assert_eq!(event.session_id(), session_id);
    }
}

#[test]
fn controller_allocates_unique_file_drag_session_ids() {
    let controller = WaylandDndController::new();

    let first = controller
        .start_file_drag(vec![PathBuf::from("/tmp/first")], test_drag_icon())
        .unwrap();
    let second = controller
        .start_file_drag(vec![PathBuf::from("/tmp/second")], test_drag_icon())
        .unwrap();

    assert_ne!(first, second);
}

#[test]
fn controller_rejects_empty_file_drag_sources() {
    let controller = WaylandDndController::new();

    let error = controller
        .start_file_drag(Vec::new(), test_drag_icon())
        .unwrap_err();

    assert!(matches!(error, WaylandDndCommandError::NoPaths));
}

#[test]
fn file_drag_icon_validates_dimensions_and_pixel_length() {
    assert!(matches!(
        WaylandFileDragIcon::new(0, 1, Vec::new()),
        Err(WaylandFileDragIconError::InvalidDimensions {
            width: 0,
            height: 1
        })
    ));
    assert!(matches!(
        WaylandFileDragIcon::new(2, 2, vec![0; 15]),
        Err(WaylandFileDragIconError::InvalidPixelLength {
            expected: 16,
            actual: 15,
            ..
        })
    ));
}
