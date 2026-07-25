use desktop_linux::WaylandFileDragIcon;
use resvg::tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};
use resvg::usvg::{Options, Tree};

use crate::icons::IconSymbol;

const DRAG_ICON_CANVAS_EDGE: u32 = 80;
const DRAG_ICON_LAYER_EDGE: u32 = 48;
const DRAG_ICON_GLYPH_EDGE: u32 = 30;
const DRAG_ICON_CURSOR_MARGIN: i32 = 10;
const DRAG_ICON_LAYER_OFFSET: i32 = 4;
const DRAG_ICON_MAX_LAYERS: usize = 4;

pub(crate) fn render_wayland_file_drag_icon(
    symbol: IconSymbol,
    source_count: usize,
) -> Result<WaylandFileDragIcon, String> {
    let glyph = render_drag_icon_glyph(symbol)?;
    let mut canvas = Pixmap::new(DRAG_ICON_CANVAS_EDGE, DRAG_ICON_CANVAS_EDGE)
        .ok_or_else(|| "could not allocate Wayland file drag icon canvas".to_owned())?;
    let layer_count = source_count.clamp(1, DRAG_ICON_MAX_LAYERS);

    for layer in (0..layer_count).rev() {
        let offset = DRAG_ICON_CURSOR_MARGIN + layer as i32 * DRAG_ICON_LAYER_OFFSET;
        draw_drag_icon_layer(&mut canvas, &glyph, offset);
    }

    WaylandFileDragIcon::new(DRAG_ICON_CANVAS_EDGE, DRAG_ICON_CANVAS_EDGE, canvas.take())
        .map_err(|error| error.to_string())
}

fn render_drag_icon_glyph(symbol: IconSymbol) -> Result<Pixmap, String> {
    let svg = String::from_utf8_lossy(symbol.bytes())
        .replace("currentColor", drag_icon_glyph_color(symbol));
    let tree = Tree::from_data(svg.as_bytes(), &Options::default())
        .map_err(|error| format!("could not parse embedded drag icon SVG: {error}"))?;
    let source_size = tree.size();
    let scale = (DRAG_ICON_GLYPH_EDGE as f32 / source_size.width())
        .min(DRAG_ICON_GLYPH_EDGE as f32 / source_size.height());
    let mut glyph = Pixmap::new(DRAG_ICON_GLYPH_EDGE, DRAG_ICON_GLYPH_EDGE)
        .ok_or_else(|| "could not allocate Wayland file drag icon glyph".to_owned())?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut glyph.as_mut(),
    );
    Ok(glyph)
}

fn draw_drag_icon_layer(canvas: &mut Pixmap, glyph: &Pixmap, offset: i32) {
    let center = offset as f32 + DRAG_ICON_LAYER_EDGE as f32 / 2.0;
    fill_circle(canvas, center + 1.0, center + 2.0, 25.0, [0, 0, 0, 80]);
    fill_circle(canvas, center, center, 24.0, [248, 250, 249, 245]);
    canvas.draw_pixmap(
        offset + (DRAG_ICON_LAYER_EDGE as i32 - DRAG_ICON_GLYPH_EDGE as i32) / 2,
        offset + (DRAG_ICON_LAYER_EDGE as i32 - DRAG_ICON_GLYPH_EDGE as i32) / 2,
        glyph.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

fn fill_circle(canvas: &mut Pixmap, x: f32, y: f32, radius: f32, rgba: [u8; 4]) {
    let Some(circle) = PathBuilder::from_circle(x, y, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    canvas.fill_path(
        &circle,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn drag_icon_glyph_color(symbol: IconSymbol) -> &'static str {
    match symbol {
        IconSymbol::Folder => "#c17d11",
        IconSymbol::TriangleAlert => "#b53c36",
        _ => "#1f6f5b",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_drag_icon_has_stable_dimensions_and_transparent_cursor_margin() {
        let icon = render_wayland_file_drag_icon(IconSymbol::File, 1).unwrap();

        assert_eq!(icon.width(), DRAG_ICON_CANVAS_EDGE);
        assert_eq!(icon.height(), DRAG_ICON_CANVAS_EDGE);
        assert_eq!(
            icon.premultiplied_rgba().len(),
            (DRAG_ICON_CANVAS_EDGE * DRAG_ICON_CANVAS_EDGE * 4) as usize
        );
        assert_eq!(&icon.premultiplied_rgba()[..4], &[0, 0, 0, 0]);
        assert!(icon
            .premultiplied_rgba()
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn multi_selection_adds_layers_but_stays_bounded() {
        let single = render_wayland_file_drag_icon(IconSymbol::Folder, 1).unwrap();
        let four = render_wayland_file_drag_icon(IconSymbol::Folder, 4).unwrap();
        let many = render_wayland_file_drag_icon(IconSymbol::Folder, 200).unwrap();

        assert_ne!(single.premultiplied_rgba(), four.premultiplied_rgba());
        assert_eq!(four.premultiplied_rgba(), many.premultiplied_rgba());
    }
}
