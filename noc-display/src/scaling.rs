//! Pure layout calculations, in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

fn centered_rect(image: (f32, f32), bounds: (f32, f32), cover: bool) -> Option<Rect> {
    let (iw, ih) = image;
    let (bw, bh) = bounds;
    if [iw, ih, bw, bh].iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let scale = if cover {
        (bw / iw).max(bh / ih)
    } else {
        (bw / iw).min(bh / ih)
    };
    let (width, height) = (iw * scale, ih * scale);
    Some(Rect {
        left: (bw - width) / 2.0,
        top: (bh - height) / 2.0,
        right: (bw + width) / 2.0,
        bottom: (bh + height) / 2.0,
    })
}

pub fn calculate_cover_rect(image: (f32, f32), viewport: (f32, f32)) -> Option<Rect> {
    centered_rect(image, viewport, true)
}

/// Fits inside the given bounds, including upscaling, preserving aspect ratio.
pub fn calculate_contain_rect(image: (f32, f32), bounds: (f32, f32)) -> Option<Rect> {
    centered_rect(image, bounds, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crops_symmetrically_without_bars() {
        assert_eq!(
            calculate_cover_rect((1000.0, 1000.0), (1920.0, 1080.0)),
            Some(Rect {
                left: 0.0,
                top: -420.0,
                right: 1920.0,
                bottom: 1500.0,
            })
        );
    }

    #[test]
    fn layouts_preserve_ratio_and_bounds_in_landscape_and_portrait() {
        for image in [(2000.0, 300.0), (300.0, 2000.0), (800.0, 800.0)] {
            for screen in [(1920.0, 1080.0), (1080.0, 1920.0), (640.0, 480.0)] {
                for cover in [true, false] {
                    let rect = centered_rect(image, screen, cover).unwrap();
                    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
                    assert!((w / h - image.0 / image.1).abs() < 0.0001);
                    assert!((rect.left + rect.right - screen.0).abs() < 0.001);
                    assert!((rect.top + rect.bottom - screen.1).abs() < 0.001);
                    if cover {
                        assert!(w >= screen.0 - 0.001 && h >= screen.1 - 0.001);
                    } else {
                        assert!(w <= screen.0 + 0.001 && h <= screen.1 + 0.001);
                    }
                }
            }
        }
    }

    #[test]
    fn contain_logo_and_invalid_sizes() {
        let r = calculate_contain_rect((1000.0, 500.0), (768.0, 432.0)).unwrap();
        assert_eq!(
            r,
            Rect {
                left: 0.0,
                top: 24.0,
                right: 768.0,
                bottom: 408.0
            }
        );
        for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(calculate_cover_rect((invalid, 10.0), (100.0, 100.0)).is_none());
            assert!(calculate_contain_rect((10.0, 10.0), (100.0, invalid)).is_none());
        }
    }
}
