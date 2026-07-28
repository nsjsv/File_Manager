use std::path::Path;

use regex::Regex;

use super::{
    BatchRenameCaseRule, BatchRenameExtensionMode, BatchRenameInsertMode, BatchRenameRandomMode,
    BatchRenameRemoveClass, BatchRenameRemoveMode, BatchRenameReplaceScope, BatchRenameSliceMode,
    BatchRenameSource, BatchRenameState,
};

pub(super) struct PreparedBatchRenameRules {
    regex: Result<Option<Regex>, String>,
    batch_commands: Result<Vec<BatchRenameCommand>, String>,
    list_names: Vec<String>,
}

enum BatchRenameCommand {
    Prefix(String),
    Suffix(String),
    Replace { find: String, replacement: String },
    Remove(String),
    Insert { position: usize, text: String },
    Slice { start: usize, length: Option<usize> },
    Case(BatchRenameCaseRule),
    Extension(Option<String>),
    Regex { pattern: Regex, replacement: String },
}

impl PreparedBatchRenameRules {
    pub(super) fn new(state: &BatchRenameState) -> Self {
        let regex = if state.regex.pattern.trim().is_empty() {
            Ok(None)
        } else {
            Regex::new(&state.regex.pattern)
                .map(Some)
                .map_err(|error| format!("invalid regex: {error}"))
        };
        let batch_commands = parse_batch_commands(&state.batch.commands);
        let list_names = parse_list_names(&state.list.names);

        Self {
            regex,
            batch_commands,
            list_names,
        }
    }

    pub(super) fn rename_item_name(
        &self,
        item: &BatchRenameSource,
        preview_index: usize,
        state: &BatchRenameState,
    ) -> Result<String, String> {
        let start = parse_usize_or_default(&state.sequence.start_input, 1);
        let step = parse_usize_or_default(&state.sequence.step_input, 1);
        let padding = parse_usize_or_default(&state.sequence.padding_input, 0);
        let sequence_number = start.saturating_add(preview_index.saturating_mul(step));
        let insert_position = parse_usize_or_default(&state.insert.position_input, 0);
        let replace_range_start = parse_optional_usize(&state.replace.range_start_input);
        let replace_range_length = parse_optional_usize(&state.replace.range_length_input);
        let slice_start = parse_optional_usize(&state.slice.start_input);
        let slice_length = parse_optional_usize(&state.slice.length_input);
        let remove_start = parse_optional_usize(&state.remove.start_input);
        let remove_length = parse_optional_usize(&state.remove.length_input);
        let random_text = deterministic_random_text(
            &item.source_name_text,
            preview_index,
            parse_usize_or_default(&state.random.length_input, 6).min(64),
            &state.random.alphabet,
        );

        let (mut stem, mut extension) = split_file_name(&item.source_name_text);
        if !state.replace.find.is_empty() {
            stem = replace_text(
                &stem,
                &state.replace.find,
                &state.replace.replacement,
                state.replace.scope,
                replace_range_start,
                replace_range_length,
                state.replace.ignore_case,
            );
        }
        if !state.insert.text.is_empty() {
            if state.insert.ignore_extension {
                stem = insert_text(&stem, state, insert_position);
            } else {
                let name = join_stem_extension(&stem, extension.as_deref());
                let name = insert_text(&name, state, insert_position);
                (stem, extension) = split_file_name(&name);
            }
        }
        if slice_start.is_some() || slice_length.is_some() {
            stem = slice_text(&stem, state, slice_start.unwrap_or(0), slice_length);
        }
        stem = remove_text(&stem, state, remove_start.unwrap_or(0), remove_length);
        stem = apply_case_rule(&stem, state.case);
        stem = apply_random_rule(stem, &random_text, state.random.mode);
        stem = apply_sequence_rule(stem, sequence_number, padding, state);

        if !state.sequence.preserve_extension {
            extension = None;
        }
        extension = apply_extension_rule(extension, state);

        let mut target_name = join_stem_extension(&stem, extension.as_deref());
        if let Some(list_name) = self.list_names.get(preview_index) {
            target_name = list_name.clone();
        }
        if !state.custom.template.is_empty() {
            target_name = render_custom_template(
                &state.custom.template,
                &target_name,
                item,
                sequence_number,
                padding,
                &random_text,
            );
        }
        if let Some(regex) = self.regex.as_ref().map_err(|error| error.clone())? {
            target_name = regex
                .replace_all(&target_name, state.regex.replacement.as_str())
                .to_string();
        }
        for command in self
            .batch_commands
            .as_ref()
            .map_err(|error| error.clone())?
        {
            target_name = apply_batch_command(target_name, command);
        }

        Ok(target_name)
    }
}

fn apply_sequence_rule(
    stem: String,
    sequence_number: usize,
    padding: usize,
    state: &BatchRenameState,
) -> String {
    if state.sequence.prefix.is_empty() && state.sequence.include_original_stem {
        return stem;
    }

    let number = padded_number(sequence_number, padding);
    let mut name = format!("{}{}", state.sequence.prefix, number);
    if state.sequence.include_original_stem && !stem.is_empty() {
        name.push(' ');
        name.push_str(&stem);
    }
    name
}

fn replace_text(
    source: &str,
    find: &str,
    replacement: &str,
    scope: BatchRenameReplaceScope,
    range_start: Option<usize>,
    range_length: Option<usize>,
    ignore_case: bool,
) -> String {
    match scope {
        BatchRenameReplaceScope::All => replace_all(source, find, replacement, ignore_case),
        BatchRenameReplaceScope::First => {
            replace_first_match(source, find, replacement, ignore_case)
                .unwrap_or_else(|| source.to_owned())
        }
        BatchRenameReplaceScope::Last => replace_last_match(source, find, replacement, ignore_case)
            .unwrap_or_else(|| source.to_owned()),
        BatchRenameReplaceScope::Range => replace_in_range(
            source,
            find,
            replacement,
            range_start.unwrap_or(0),
            range_length,
            ignore_case,
        ),
    }
}

fn insert_text(source: &str, state: &BatchRenameState, position: usize) -> String {
    match state.insert.mode {
        BatchRenameInsertMode::Before => format!("{}{}", state.insert.text, source),
        BatchRenameInsertMode::After => format!("{}{}", source, state.insert.text),
        BatchRenameInsertMode::Position => {
            insert_text_at_char_position(source, &state.insert.text, position)
        }
        BatchRenameInsertMode::AfterAnchor => {
            insert_text_after_anchor(source, &state.insert.anchor, &state.insert.text)
        }
    }
}

fn slice_text(
    source: &str,
    state: &BatchRenameState,
    start: usize,
    length: Option<usize>,
) -> String {
    match state.slice.mode {
        BatchRenameSliceMode::Position => slice_text_by_chars(source, start, length),
        BatchRenameSliceMode::AfterAnchor => {
            slice_text_after_anchor(source, &state.slice.anchor, length)
        }
    }
}

fn remove_text(
    source: &str,
    state: &BatchRenameState,
    start: usize,
    length: Option<usize>,
) -> String {
    match state.remove.mode {
        BatchRenameRemoveMode::TextAndRange => {
            let mut next = source.to_owned();
            if !state.remove.text.is_empty() {
                next = next.replace(&state.remove.text, "");
            }
            if !state.remove.start_input.trim().is_empty()
                || !state.remove.length_input.trim().is_empty()
            {
                next = remove_text_by_char_range(&next, start, length);
            }
            next
        }
        BatchRenameRemoveMode::CharacterClasses => {
            remove_char_classes(source, &state.remove.classes)
        }
    }
}

fn apply_extension_rule(extension: Option<String>, state: &BatchRenameState) -> Option<String> {
    match state.extension.mode {
        BatchRenameExtensionMode::Preserve => extension,
        BatchRenameExtensionMode::Remove => None,
        BatchRenameExtensionMode::Replace => normalize_extension(&state.extension.replacement),
        BatchRenameExtensionMode::Lowercase => extension.map(|extension| extension.to_lowercase()),
        BatchRenameExtensionMode::Uppercase => extension.map(|extension| extension.to_uppercase()),
    }
}

fn apply_random_rule(stem: String, random_text: &str, mode: BatchRenameRandomMode) -> String {
    match mode {
        BatchRenameRandomMode::Off => stem,
        BatchRenameRandomMode::ReplaceStem => random_text.to_owned(),
        BatchRenameRandomMode::Prefix => format!("{random_text}{stem}"),
        BatchRenameRandomMode::Suffix => format!("{stem}{random_text}"),
    }
}

fn render_custom_template(
    template: &str,
    current_name: &str,
    item: &BatchRenameSource,
    sequence_number: usize,
    padding: usize,
    random_text: &str,
) -> String {
    let (current_stem, current_extension) = split_file_name(current_name);
    let (original_stem, original_extension) = split_file_name(&item.source_name_text);
    template
        .replace("{name}", current_name)
        .replace("{stem}", &current_stem)
        .replace("{ext}", current_extension.as_deref().unwrap_or(""))
        .replace("{index}", &sequence_number.to_string())
        .replace("{n}", &padded_number(sequence_number, padding))
        .replace("{original}", &item.source_name_text)
        .replace("{original_stem}", &original_stem)
        .replace(
            "{original_ext}",
            original_extension.as_deref().unwrap_or(""),
        )
        .replace("{random}", random_text)
}

fn apply_batch_command(current_name: String, command: &BatchRenameCommand) -> String {
    match command {
        BatchRenameCommand::Prefix(prefix) => {
            let (stem, extension) = split_file_name(&current_name);
            join_stem_extension(&format!("{prefix}{stem}"), extension.as_deref())
        }
        BatchRenameCommand::Suffix(suffix) => {
            let (stem, extension) = split_file_name(&current_name);
            join_stem_extension(&format!("{stem}{suffix}"), extension.as_deref())
        }
        BatchRenameCommand::Replace { find, replacement } => {
            current_name.replace(find, replacement)
        }
        BatchRenameCommand::Remove(text) => current_name.replace(text, ""),
        BatchRenameCommand::Insert { position, text } => {
            insert_text_at_char_position(&current_name, text, *position)
        }
        BatchRenameCommand::Slice { start, length } => {
            slice_text_by_chars(&current_name, *start, *length)
        }
        BatchRenameCommand::Case(case) => {
            let (stem, extension) = split_file_name(&current_name);
            join_stem_extension(&apply_case_rule(&stem, *case), extension.as_deref())
        }
        BatchRenameCommand::Extension(extension) => {
            let (stem, _) = split_file_name(&current_name);
            join_stem_extension(&stem, extension.as_deref())
        }
        BatchRenameCommand::Regex {
            pattern,
            replacement,
        } => pattern
            .replace_all(&current_name, replacement.as_str())
            .to_string(),
    }
}

fn parse_batch_commands(input: &str) -> Result<Vec<BatchRenameCommand>, String> {
    let mut commands = Vec::new();
    for raw_command in input.lines().flat_map(|line| line.split(';')) {
        let command = raw_command.trim();
        if command.is_empty() {
            continue;
        }
        commands.push(parse_batch_command(command)?);
    }
    Ok(commands)
}

fn parse_batch_command(command: &str) -> Result<BatchRenameCommand, String> {
    let mut words = command.splitn(2, char::is_whitespace);
    let keyword = words.next().unwrap_or("").to_ascii_lowercase();
    let rest = words.next().unwrap_or("").trim();

    match keyword.as_str() {
        "prefix" if !rest.is_empty() => Ok(BatchRenameCommand::Prefix(rest.to_owned())),
        "suffix" if !rest.is_empty() => Ok(BatchRenameCommand::Suffix(rest.to_owned())),
        "replace" => {
            let (find, replacement) = split_rule_arrow(rest, "replace")?;
            Ok(BatchRenameCommand::Replace { find, replacement })
        }
        "remove" if !rest.is_empty() => Ok(BatchRenameCommand::Remove(rest.to_owned())),
        "insert" => {
            let (position, text) = split_position_text(rest, "insert")?;
            Ok(BatchRenameCommand::Insert { position, text })
        }
        "slice" => {
            let mut parts = rest.split_whitespace();
            let start = parse_required_usize(parts.next(), "slice start")?;
            let length = parts
                .next()
                .map(|value| parse_required_usize(Some(value), "slice length"))
                .transpose()?;
            Ok(BatchRenameCommand::Slice { start, length })
        }
        "case" => parse_batch_case_command(rest),
        "ext" | "extension" => Ok(BatchRenameCommand::Extension(normalize_extension(rest))),
        "regex" => {
            let (pattern, replacement) = split_rule_arrow(rest, "regex")?;
            let pattern =
                Regex::new(&pattern).map_err(|error| format!("invalid regex command: {error}"))?;
            Ok(BatchRenameCommand::Regex {
                pattern,
                replacement,
            })
        }
        _ => Err(format!("unsupported batch command: {command}")),
    }
}

fn parse_batch_case_command(rest: &str) -> Result<BatchRenameCommand, String> {
    let case = match rest.to_ascii_lowercase().as_str() {
        "lower" | "lowercase" => BatchRenameCaseRule::Lowercase,
        "upper" | "uppercase" => BatchRenameCaseRule::Uppercase,
        "title" | "titlecase" => BatchRenameCaseRule::TitleCase,
        "invert" | "swap" => BatchRenameCaseRule::InvertCase,
        "keep" | "unchanged" => BatchRenameCaseRule::Unchanged,
        _ => return Err(format!("unsupported case command: {rest}")),
    };
    Ok(BatchRenameCommand::Case(case))
}

fn split_rule_arrow(rest: &str, command: &str) -> Result<(String, String), String> {
    let Some((left, right)) = rest.split_once("=>") else {
        return Err(format!("{command} command requires =>"));
    };
    let find = left.trim().to_owned();
    if find.is_empty() {
        return Err(format!("{command} command requires a pattern"));
    }
    Ok((find, right.trim().to_owned()))
}

fn split_position_text(rest: &str, command: &str) -> Result<(usize, String), String> {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let position = parse_required_usize(parts.next(), command)?;
    let text = parts.next().unwrap_or("").trim();
    if text.is_empty() {
        return Err(format!("{command} command requires text"));
    }
    Ok((position, text.to_owned()))
}

fn parse_required_usize(value: Option<&str>, label: &str) -> Result<usize, String> {
    value
        .ok_or_else(|| format!("{label} is missing"))?
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a number"))
}

fn parse_list_names(input: &str) -> Vec<String> {
    if input.trim().is_empty() {
        return Vec::new();
    }

    let names = if input.contains('\n') {
        input.lines().collect::<Vec<_>>()
    } else {
        input.split('|').collect::<Vec<_>>()
    };
    names
        .into_iter()
        .map(|name| name.trim_matches('\r').trim().to_owned())
        .collect()
}

fn normalize_extension(input: &str) -> Option<String> {
    let extension = input.trim().trim_start_matches('.');
    if extension.is_empty() || extension.eq_ignore_ascii_case("remove") {
        None
    } else {
        Some(extension.to_owned())
    }
}

fn split_file_name(name: &str) -> (String, Option<String>) {
    let path = Path::new(name);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(ToOwned::to_owned);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name)
        .to_owned();
    (stem, extension)
}

fn join_stem_extension(stem: &str, extension: Option<&str>) -> String {
    match extension {
        Some(extension) if !extension.is_empty() => format!("{stem}.{extension}"),
        _ => stem.to_owned(),
    }
}

fn insert_text_at_char_position(source: &str, text: &str, position: usize) -> String {
    let byte_position = source
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(source.len()))
        .nth(position)
        .unwrap_or(source.len());
    let mut next = String::with_capacity(source.len() + text.len());
    next.push_str(&source[..byte_position]);
    next.push_str(text);
    next.push_str(&source[byte_position..]);
    next
}

fn insert_text_after_anchor(source: &str, anchor: &str, text: &str) -> String {
    if anchor.is_empty() {
        return format!("{source}{text}");
    }
    let Some(byte_position) = source.find(anchor).map(|index| index + anchor.len()) else {
        return format!("{source}{text}");
    };
    let mut next = String::with_capacity(source.len() + text.len());
    next.push_str(&source[..byte_position]);
    next.push_str(text);
    next.push_str(&source[byte_position..]);
    next
}

fn slice_text_by_chars(source: &str, start: usize, length: Option<usize>) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    if start >= chars.len() {
        return String::new();
    }
    let end = length
        .map(|length| start.saturating_add(length).min(chars.len()))
        .unwrap_or(chars.len());
    chars[start..end].iter().collect()
}

fn slice_text_after_anchor(source: &str, anchor: &str, length: Option<usize>) -> String {
    if anchor.is_empty() {
        return String::new();
    }
    let Some(start) = source
        .find(anchor)
        .map(|index| source[..index + anchor.len()].chars().count())
    else {
        return String::new();
    };
    slice_text_by_chars(source, start, length)
}

fn remove_text_by_char_range(source: &str, start: usize, length: Option<usize>) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    if start >= chars.len() {
        return source.to_owned();
    }
    let end = length
        .map(|length| start.saturating_add(length).min(chars.len()))
        .unwrap_or(chars.len());
    chars
        .iter()
        .enumerate()
        .filter_map(|(index, character)| {
            if (start..end).contains(&index) {
                None
            } else {
                Some(*character)
            }
        })
        .collect()
}

fn remove_char_classes(source: &str, classes: &[BatchRenameRemoveClass]) -> String {
    source
        .chars()
        .filter(|character| {
            !classes
                .iter()
                .any(|class| remove_class_matches(*class, *character))
        })
        .collect()
}

fn remove_class_matches(class: BatchRenameRemoveClass, character: char) -> bool {
    match class {
        BatchRenameRemoveClass::Lowercase => character.is_lowercase(),
        BatchRenameRemoveClass::Uppercase => character.is_uppercase(),
        BatchRenameRemoveClass::Digits => character.is_ascii_digit(),
        BatchRenameRemoveClass::Symbols => {
            !character.is_alphanumeric() && !character.is_whitespace() && !is_bracket(character)
        }
        BatchRenameRemoveClass::Brackets => is_bracket(character),
        BatchRenameRemoveClass::Whitespace => character.is_whitespace(),
        BatchRenameRemoveClass::Hanzi => is_hanzi(character),
    }
}

fn is_bracket(character: char) -> bool {
    matches!(character, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>')
}

fn is_hanzi(character: char) -> bool {
    matches!(character as u32,
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF |
        0xF900..=0xFAFF |
        0x20000..=0x2A6DF |
        0x2A700..=0x2B73F |
        0x2B740..=0x2B81F |
        0x2B820..=0x2CEAF |
        0x2CEB0..=0x2EBEF |
        0x30000..=0x3134F)
}

fn replace_all(source: &str, find: &str, replacement: &str, ignore_case: bool) -> String {
    if !ignore_case {
        return source.replace(find, replacement);
    }
    replace_all_case_insensitive(source, find, replacement)
}

fn replace_all_case_insensitive(source: &str, find: &str, replacement: &str) -> String {
    let mut next = String::new();
    let mut remainder = source;
    while let Some((start, end)) = find_case_insensitive(remainder, find) {
        next.push_str(&remainder[..start]);
        next.push_str(replacement);
        remainder = &remainder[end..];
    }
    next.push_str(remainder);
    next
}

fn replace_first_match(
    source: &str,
    find: &str,
    replacement: &str,
    ignore_case: bool,
) -> Option<String> {
    let (start, end) = if ignore_case {
        find_case_insensitive(source, find)?
    } else {
        let start = source.find(find)?;
        (start, start + find.len())
    };
    Some(replace_byte_range(source, start, end, replacement))
}

fn replace_last_match(
    source: &str,
    find: &str,
    replacement: &str,
    ignore_case: bool,
) -> Option<String> {
    let (start, end) = if ignore_case {
        find_last_case_insensitive(source, find)?
    } else {
        let start = source.rfind(find)?;
        (start, start + find.len())
    };
    Some(replace_byte_range(source, start, end, replacement))
}

fn replace_in_range(
    source: &str,
    find: &str,
    replacement: &str,
    start: usize,
    length: Option<usize>,
    ignore_case: bool,
) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    if start >= chars.len() {
        return source.to_owned();
    }
    let end = length
        .map(|value| start.saturating_add(value).min(chars.len()))
        .unwrap_or(chars.len());
    let prefix: String = chars[..start].iter().collect();
    let middle: String = chars[start..end].iter().collect();
    let suffix: String = chars[end..].iter().collect();
    format!(
        "{}{}{}",
        prefix,
        replace_all(&middle, find, replacement, ignore_case),
        suffix
    )
}

fn replace_byte_range(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut next = String::with_capacity(source.len() + replacement.len());
    next.push_str(&source[..start]);
    next.push_str(replacement);
    next.push_str(&source[end..]);
    next
}

fn find_case_insensitive(source: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    for (start, _) in source.char_indices() {
        let end = source[start..]
            .char_indices()
            .nth(needle.chars().count())
            .map(|(offset, _)| start + offset)
            .unwrap_or(source.len());
        if case_insensitive_match(&source[start..end], needle) {
            return Some((start, end));
        }
    }
    None
}

fn find_last_case_insensitive(source: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let mut last = None;
    for (start, _) in source.char_indices() {
        let end = source[start..]
            .char_indices()
            .nth(needle.chars().count())
            .map(|(offset, _)| start + offset)
            .unwrap_or(source.len());
        if case_insensitive_match(&source[start..end], needle) {
            last = Some((start, end));
        }
    }
    last
}

fn case_insensitive_match(source: &str, needle: &str) -> bool {
    source
        .chars()
        .flat_map(|character| character.to_lowercase())
        .eq(needle
            .chars()
            .flat_map(|character| character.to_lowercase()))
}

fn apply_case_rule(source: &str, case: BatchRenameCaseRule) -> String {
    match case {
        BatchRenameCaseRule::Unchanged => source.to_owned(),
        BatchRenameCaseRule::Lowercase => source.to_lowercase(),
        BatchRenameCaseRule::Uppercase => source.to_uppercase(),
        BatchRenameCaseRule::TitleCase => title_case(source),
        BatchRenameCaseRule::InvertCase => invert_case(source),
    }
}

fn title_case(source: &str) -> String {
    let mut next_word = true;
    let mut output = String::new();
    for character in source.chars() {
        if character.is_alphanumeric() {
            if next_word {
                output.extend(character.to_uppercase());
                next_word = false;
            } else {
                output.extend(character.to_lowercase());
            }
        } else {
            output.push(character);
            next_word = true;
        }
    }
    output
}

fn invert_case(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    for character in source.chars() {
        if character.is_lowercase() {
            output.extend(character.to_uppercase());
        } else if character.is_uppercase() {
            output.extend(character.to_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn deterministic_random_text(name: &str, index: usize, length: usize, alphabet: &str) -> String {
    let alphabet = alphabet.chars().collect::<Vec<_>>();
    let alphabet = if alphabet.is_empty() {
        "abcdefghijklmnopqrstuvwxyz0123456789"
            .chars()
            .collect::<Vec<_>>()
    } else {
        alphabet
    };
    let mut hasher = blake3::Hasher::new();
    hasher.update(name.as_bytes());
    hasher.update(&index.to_le_bytes());
    let hash = hasher.finalize();
    let mut seed_bytes = [0u8; 8];
    seed_bytes.copy_from_slice(&hash.as_bytes()[..8]);
    let mut seed = u64::from_le_bytes(seed_bytes);

    let mut output = String::new();
    for _ in 0..length {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let character = alphabet[(seed as usize) % alphabet.len()];
        output.push(character);
    }
    output
}

fn padded_number(number: usize, padding: usize) -> String {
    if padding == 0 {
        number.to_string()
    } else {
        format!("{number:0padding$}")
    }
}

fn parse_usize_or_default(input: &str, default: usize) -> usize {
    input.trim().parse::<usize>().unwrap_or(default)
}

fn parse_optional_usize(input: &str) -> Option<usize> {
    let input = input.trim();
    if input.is_empty() {
        None
    } else {
        input.parse::<usize>().ok()
    }
}
