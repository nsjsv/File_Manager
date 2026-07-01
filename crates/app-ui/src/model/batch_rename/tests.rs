use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use super::*;

fn state_for_names(names: &[&str]) -> BatchRenameState {
    state_for_sources(
        names
            .iter()
            .map(|name| BatchRenameSource {
                path: PathBuf::from("/tmp").join(name),
                name: (*name).to_owned(),
                modified: None,
            })
            .collect(),
    )
}

fn source_with_modified(name: &str, modified_secs: u64) -> BatchRenameSource {
    BatchRenameSource {
        path: PathBuf::from("/tmp").join(name),
        name: name.to_owned(),
        modified: Some(UNIX_EPOCH + Duration::from_secs(modified_secs)),
    }
}

fn state_for_sources(items: Vec<BatchRenameSource>) -> BatchRenameState {
    BatchRenameState::new_with_existing_sources(items, HashSet::new()).unwrap()
}

fn preview_names(state: &BatchRenameState) -> Vec<&str> {
    state
        .preview
        .rows
        .iter()
        .map(|row| row.target_name.as_str())
        .collect()
}

fn preview_original_names(state: &BatchRenameState) -> Vec<&str> {
    state
        .preview
        .rows
        .iter()
        .map(|row| row.original_name.as_str())
        .collect()
}

#[test]
fn batch_rename_sequence_prefixes_number_and_preserves_extension() {
    let mut state = state_for_names(&["report.txt", "notes.txt"]);
    state.sequence.prefix = "File ".to_owned();
    state.sequence.start_input = "3".to_owned();
    state.sequence.padding_input = "3".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["File 003 report.txt", "File 004 notes.txt"]
    );
}

#[test]
fn batch_rename_sequence_respects_step() {
    let mut state = state_for_names(&["report.txt", "notes.txt"]);
    state.sequence.prefix = "File ".to_owned();
    state.sequence.start_input = "3".to_owned();
    state.sequence.step_input = "5".to_owned();
    state.sequence.padding_input = "3".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["File 003 report.txt", "File 008 notes.txt"]
    );
}

#[test]
fn batch_rename_replace_insert_slice_and_case_are_ordered() {
    let mut state = state_for_names(&["summer draft.txt", "winter draft.txt"]);
    state.replace.find = "draft".to_owned();
    state.replace.replacement = "photo".to_owned();
    state.insert.text = "2026 ".to_owned();
    state.insert.position_input = "0".to_owned();
    state.slice.start_input = "0".to_owned();
    state.slice.length_input = "11".to_owned();
    state.case = BatchRenameCaseRule::Uppercase;
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["2026 SUMMER.txt", "2026 WINTER.txt"]
    );
}

#[test]
fn batch_rename_replace_supports_first_last_range_and_ignore_case() {
    let mut state = state_for_names(&["foo-foo-FOO.txt", "demo.txt"]);
    state.replace.find = "foo".to_owned();
    state.replace.replacement = "bar".to_owned();
    state.replace.scope = BatchRenameReplaceScope::First;
    state.rebuild_preview();
    assert_eq!(preview_names(&state)[0], "bar-foo-FOO.txt");

    state.replace.scope = BatchRenameReplaceScope::Last;
    state.replace.ignore_case = true;
    state.rebuild_preview();
    assert_eq!(preview_names(&state)[0], "foo-foo-bar.txt");

    state.replace.scope = BatchRenameReplaceScope::Range;
    state.replace.ignore_case = false;
    state.replace.range_start_input = "4".to_owned();
    state.replace.range_length_input = "7".to_owned();
    state.rebuild_preview();
    assert_eq!(preview_names(&state)[0], "foo-bar-FOO.txt");
}

#[test]
fn batch_rename_insert_supports_after_anchor() {
    let mut state = state_for_names(&["photo_final.txt", "notes.txt"]);
    state.insert.mode = BatchRenameInsertMode::AfterAnchor;
    state.insert.anchor = "photo".to_owned();
    state.insert.text = "-2026".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["photo-2026_final.txt", "notes-2026.txt"]
    );
}

#[test]
fn batch_rename_slice_supports_after_anchor() {
    let mut state = state_for_names(&["photo_final.txt", "notes.txt"]);
    state.slice.mode = BatchRenameSliceMode::AfterAnchor;
    state.slice.anchor = "photo_".to_owned();
    state.slice.length_input = "5".to_owned();
    state.rebuild_preview();

    assert_eq!(preview_names(&state), vec!["final.txt", ".txt"]);
}

#[test]
fn batch_rename_case_invert_flips_stem_letter_case() {
    let mut state = state_for_names(&["AbC.txt", "xYz.txt"]);
    state.case = BatchRenameCaseRule::InvertCase;
    state.rebuild_preview();

    assert_eq!(preview_names(&state), vec!["aBc.txt", "XyZ.txt"]);
}

#[test]
fn batch_rename_sort_changes_sequence_order() {
    let mut state = state_for_names(&["b.txt", "a.txt"]);
    state.sort.mode = BatchRenameSortMode::NameAscending;
    state.sequence.prefix = "Item ".to_owned();
    state.sequence.include_original_stem = false;
    state.rebuild_preview();

    assert_eq!(preview_names(&state), vec!["Item 01.txt", "Item 02.txt"]);
    assert_eq!(state.preview.rows[0].original_name, "a.txt");
}

#[test]
fn batch_rename_natural_sort_orders_embedded_numbers() {
    let mut state = state_for_names(&["file10.txt", "file2.txt", "file1.txt"]);
    state.sort.mode = BatchRenameSortMode::NaturalAscending;
    state.rebuild_preview();

    assert_eq!(
        preview_original_names(&state),
        vec!["file1.txt", "file2.txt", "file10.txt"]
    );
}

#[test]
fn batch_rename_modified_sort_uses_source_metadata() {
    let mut state = state_for_sources(vec![
        source_with_modified("middle.txt", 20),
        source_with_modified("oldest.txt", 10),
        source_with_modified("newest.txt", 30),
    ]);
    state.sort.mode = BatchRenameSortMode::ModifiedAscending;
    state.rebuild_preview();

    assert_eq!(
        preview_original_names(&state),
        vec!["oldest.txt", "middle.txt", "newest.txt"]
    );

    state.sort.mode = BatchRenameSortMode::ModifiedDescending;
    state.rebuild_preview();

    assert_eq!(
        preview_original_names(&state),
        vec!["newest.txt", "middle.txt", "oldest.txt"]
    );
}

#[test]
fn batch_rename_random_sort_is_deterministic() {
    let mut state = state_for_sources(vec![
        source_with_modified("alpha.txt", 10),
        source_with_modified("beta.txt", 20),
        source_with_modified("gamma.txt", 30),
    ]);
    state.sort.mode = BatchRenameSortMode::Random;
    state.rebuild_preview();
    let first_order = preview_original_names(&state)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    state.rebuild_preview();

    assert_eq!(preview_original_names(&state), first_order);
    assert_ne!(first_order, vec!["alpha.txt", "beta.txt", "gamma.txt"]);
}

#[test]
fn batch_rename_preview_drag_reorders_selection_order() {
    let mut state = state_for_names(&["b.txt", "a.txt", "c.txt"]);
    state.sort.mode = BatchRenameSortMode::NameAscending;
    state.rebuild_preview();

    assert_eq!(
        preview_original_names(&state),
        vec!["a.txt", "b.txt", "c.txt"]
    );

    let source = PathBuf::from("/tmp/a.txt");
    state.apply_update(BatchRenameMessage::PreviewDragStarted(source.clone()));
    state.apply_update(BatchRenameMessage::PreviewDragEntered(PathBuf::from(
        "/tmp/c.txt",
    )));
    state.rebuild_preview();

    assert_eq!(state.sort.mode, BatchRenameSortMode::SelectionOrder);
    assert_eq!(
        preview_original_names(&state),
        vec!["b.txt", "c.txt", "a.txt"]
    );
    assert_eq!(state.dragging_preview_source(), Some(source.as_path()));

    state.apply_update(BatchRenameMessage::PreviewDragFinished);
    assert_eq!(state.dragging_preview_source(), None);
}

#[test]
fn batch_rename_preview_name_edit_overrides_single_target() {
    let mut state = state_for_names(&["alpha.txt", "beta.txt"]);
    let source = PathBuf::from("/tmp/beta.txt");

    state.apply_update(BatchRenameMessage::PreviewNameEditStarted(source.clone()));
    assert_eq!(state.editing_target_name_source(), Some(source.as_path()));
    assert_eq!(state.editing_target_name_input(), "beta.txt");

    state.apply_update(BatchRenameMessage::PreviewNameChanged(
        "chosen.md".to_owned(),
    ));
    state.rebuild_preview();
    assert_eq!(preview_names(&state), vec!["alpha.txt", "chosen.md"]);

    state.replace.find = "alpha".to_owned();
    state.replace.replacement = "renamed".to_owned();
    state.rebuild_preview();
    assert_eq!(preview_names(&state), vec!["renamed.txt", "chosen.md"]);

    state.apply_update(BatchRenameMessage::PreviewNameEditCommitted);
    assert_eq!(state.editing_target_name_source(), None);
}

#[test]
fn batch_rename_extension_modes_transform_extension() {
    let mut state = state_for_names(&["photo.JPEG", "archive.tar"]);
    state.extension.mode = BatchRenameExtensionMode::Replace;
    state.extension.replacement = ".bak".to_owned();
    state.rebuild_preview();
    assert_eq!(preview_names(&state), vec!["photo.bak", "archive.bak"]);

    state.extension.mode = BatchRenameExtensionMode::Remove;
    state.rebuild_preview();
    assert_eq!(preview_names(&state), vec!["photo", "archive"]);
}

#[test]
fn batch_rename_random_is_deterministic() {
    let mut state = state_for_names(&["photo.jpg", "photo.jpg"]);
    state.random.mode = BatchRenameRandomMode::Suffix;
    state.random.length_input = "4".to_owned();
    state.random.alphabet = "ab".to_owned();
    state.rebuild_preview();
    let first_preview = preview_names(&state)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    state.rebuild_preview();
    assert_eq!(preview_names(&state), first_preview);
    assert!(first_preview.iter().all(|name| name.ends_with(".jpg")));
}

#[test]
fn batch_rename_remove_text_and_range() {
    let mut state = state_for_names(&["draft-2026-final.txt", "draft-2027-final.txt"]);
    state.remove.text = "draft-".to_owned();
    state.remove.start_input = "4".to_owned();
    state.remove.length_input = "1".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["2026final.txt", "2027final.txt"]
    );
}

#[test]
fn batch_rename_remove_character_classes() {
    let mut state = state_for_names(&["Ab 中-12(测).txt", "xY 9!.txt"]);
    state.remove.mode = BatchRenameRemoveMode::CharacterClasses;
    state.remove.classes = vec![
        BatchRenameRemoveClass::Uppercase,
        BatchRenameRemoveClass::Digits,
        BatchRenameRemoveClass::Symbols,
        BatchRenameRemoveClass::Brackets,
        BatchRenameRemoveClass::Whitespace,
        BatchRenameRemoveClass::Hanzi,
    ];
    state.rebuild_preview();

    assert_eq!(preview_names(&state), vec!["b.txt", "x.txt"]);
}

#[test]
fn batch_rename_list_overrides_target_names_by_order() {
    let mut state = state_for_names(&["b.txt", "a.txt"]);
    state.sort.mode = BatchRenameSortMode::NameAscending;
    state.list.names = "first.md|second.md".to_owned();
    state.rebuild_preview();

    assert_eq!(preview_names(&state), vec!["first.md", "second.md"]);
}

#[test]
fn batch_rename_custom_template_uses_placeholders() {
    let mut state = state_for_names(&["report.txt", "notes.md"]);
    state.sequence.start_input = "7".to_owned();
    state.sequence.padding_input = "3".to_owned();
    state.custom.template = "{n}-{original_stem}.{ext}".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["007-report.txt", "008-notes.md"]
    );
}

#[test]
fn batch_rename_regex_replaces_whole_target_name() {
    let mut state = state_for_names(&["IMG_0001.JPG", "IMG_0002.JPG"]);
    state.regex.pattern = r"IMG_(\d+)".to_owned();
    state.regex.replacement = "photo-$1".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["photo-0001.JPG", "photo-0002.JPG"]
    );
}

#[test]
fn batch_rename_batch_commands_apply_in_order() {
    let mut state = state_for_names(&["draft.txt", "notes.txt"]);
    state.batch.commands = "prefix final-; replace draft => copy; ext md".to_owned();
    state.rebuild_preview();

    assert_eq!(
        preview_names(&state),
        vec!["final-copy.md", "final-notes.md"]
    );
}

#[test]
fn batch_rename_invalid_regex_blocks_apply() {
    let mut state = state_for_names(&["a.txt", "b.txt"]);
    state.regex.pattern = "(".to_owned();
    state.rebuild_preview();

    assert!(state
        .preview
        .rows
        .iter()
        .all(|row| row.status == BatchRenamePreviewStatus::RuleError));
    assert!(!state.can_apply());
}

#[test]
fn batch_rename_preview_marks_duplicate_targets() {
    let mut state = state_for_names(&["a.txt", "b.txt"]);
    state.slice.start_input = "99".to_owned();
    state.rebuild_preview();

    assert!(state
        .preview
        .rows
        .iter()
        .all(|row| row.status == BatchRenamePreviewStatus::DuplicateTarget));
    assert!(!state.can_apply());
}

#[test]
fn batch_rename_preview_marks_existing_unselected_target() {
    let existing = [PathBuf::from("/tmp/taken.txt")]
        .into_iter()
        .collect::<HashSet<_>>();
    let mut state = BatchRenameState::new_with_existing_sources(
        vec![
            BatchRenameSource {
                path: PathBuf::from("/tmp/report.txt"),
                name: "report.txt".to_owned(),
                modified: None,
            },
            BatchRenameSource {
                path: PathBuf::from("/tmp/notes.txt"),
                name: "notes.txt".to_owned(),
                modified: None,
            },
        ],
        existing,
    )
    .unwrap();
    state.replace.find = "report".to_owned();
    state.replace.replacement = "taken".to_owned();
    state.rebuild_preview();

    assert_eq!(
        state.preview.rows[0].status,
        BatchRenamePreviewStatus::ExistingTarget
    );
    assert!(!state.can_apply());
}
