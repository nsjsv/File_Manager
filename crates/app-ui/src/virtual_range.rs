#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VirtualRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) before_height: f32,
    pub(crate) after_height: f32,
}

impl VirtualRange {
    pub(crate) fn empty() -> Self {
        Self {
            start: 0,
            end: 0,
            before_height: 0.0,
            after_height: 0.0,
        }
    }
}

pub(crate) fn virtual_range_for_viewport(
    total_rows: usize,
    row_height: f32,
    offset_y: f32,
    viewport_height: f32,
    overscan_rows: usize,
) -> VirtualRange {
    if total_rows == 0 || row_height <= f32::EPSILON {
        return VirtualRange::empty();
    }

    let first_visible = (offset_y.max(0.0) / row_height).floor() as usize;
    let visible_rows = (viewport_height.max(row_height) / row_height).ceil() as usize;
    let start = first_visible.saturating_sub(overscan_rows).min(total_rows);
    let end = first_visible
        .saturating_add(visible_rows)
        .saturating_add(overscan_rows)
        .min(total_rows)
        .max(start);

    heights_for_range(total_rows, row_height, start, end)
}

pub(crate) fn initial_virtual_range(
    total_rows: usize,
    row_height: f32,
    initial_rows: usize,
) -> VirtualRange {
    if total_rows == 0 || row_height <= f32::EPSILON {
        return VirtualRange::empty();
    }
    heights_for_range(total_rows, row_height, 0, initial_rows.min(total_rows))
}

fn heights_for_range(total_rows: usize, row_height: f32, start: usize, end: usize) -> VirtualRange {
    VirtualRange {
        start,
        end,
        before_height: start as f32 * row_height,
        after_height: total_rows.saturating_sub(end) as f32 * row_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_at_start_includes_overscan() {
        let range = virtual_range_for_viewport(100, 10.0, 0.0, 30.0, 2);

        assert_eq!(range.start, 0);
        assert_eq!(range.end, 5);
        assert_eq!(range.before_height, 0.0);
        assert_eq!(range.after_height, 950.0);
    }

    #[test]
    fn viewport_in_middle_has_spacers() {
        let range = virtual_range_for_viewport(100, 10.0, 500.0, 30.0, 2);

        assert_eq!(range.start, 48);
        assert_eq!(range.end, 55);
        assert_eq!(range.before_height, 480.0);
        assert_eq!(range.after_height, 450.0);
    }

    #[test]
    fn viewport_near_end_clamps_end() {
        let range = virtual_range_for_viewport(10, 10.0, 95.0, 30.0, 2);

        assert_eq!(range.start, 7);
        assert_eq!(range.end, 10);
        assert_eq!(range.after_height, 0.0);
    }

    #[test]
    fn separated_row_stride_keeps_rendered_content_height_stable() {
        let row_height = 24.0;
        let row_gap = 2.0;
        let row_stride = row_height + row_gap;
        let first_range = virtual_range_for_viewport(120, row_stride, row_stride * 40.0, 180.0, 16);
        let second_range =
            virtual_range_for_viewport(120, row_stride, row_stride * 85.0, 180.0, 16);

        assert_eq!(
            rendered_height_with_spacer_gaps(first_range, row_height, row_gap),
            rendered_height_with_spacer_gaps(second_range, row_height, row_gap)
        );
    }

    #[test]
    fn initial_range_limits_first_render() {
        let range = initial_virtual_range(100, 10.0, 12);

        assert_eq!(range.start, 0);
        assert_eq!(range.end, 12);
        assert_eq!(range.after_height, 880.0);
    }

    #[test]
    fn empty_rows_return_empty_range() {
        assert_eq!(
            virtual_range_for_viewport(0, 10.0, 0.0, 30.0, 2),
            VirtualRange::empty()
        );
        assert_eq!(initial_virtual_range(0, 10.0, 12), VirtualRange::empty());
    }

    #[test]
    fn invalid_row_height_returns_empty_range() {
        assert_eq!(
            virtual_range_for_viewport(100, 0.0, 0.0, 30.0, 2),
            VirtualRange::empty()
        );
        assert_eq!(initial_virtual_range(100, 0.0, 12), VirtualRange::empty());
    }

    fn rendered_height_with_spacer_gaps(range: VirtualRange, row_height: f32, row_gap: f32) -> f32 {
        let rendered_rows = range.end.saturating_sub(range.start) as f32;
        range.before_height
            + range.after_height
            + rendered_rows * row_height
            + (rendered_rows + 1.0) * row_gap
    }
}
