use image::{Rgba, RgbaImage};

/// Extrude edge pixels outward by `amount` pixels.
/// This prevents texture bleeding when using bilinear filtering.
/// The returned image is `(w + 2*amount, h + 2*amount)`.
pub fn extrude_edges(img: &RgbaImage, amount: u32) -> RgbaImage {
    if amount == 0 {
        return img.clone();
    }

    let (w, h) = img.dimensions();
    let new_w = w + amount * 2;
    let new_h = h + amount * 2;
    let mut result = RgbaImage::new(new_w, new_h);

    // Copy the original image into the center
    image::imageops::overlay(&mut result, img, amount as i64, amount as i64);

    // Extrude top edge
    for dy in 0..amount {
        for x in 0..w {
            let pixel = *img.get_pixel(x, 0);
            result.put_pixel(x + amount, dy, pixel);
        }
    }

    // Extrude bottom edge
    for dy in 0..amount {
        for x in 0..w {
            let pixel = *img.get_pixel(x, h - 1);
            result.put_pixel(x + amount, amount + h + dy, pixel);
        }
    }

    // Extrude left edge
    for y in 0..h {
        let pixel = *img.get_pixel(0, y);
        for dx in 0..amount {
            result.put_pixel(dx, y + amount, pixel);
        }
    }

    // Extrude right edge
    for y in 0..h {
        let pixel = *img.get_pixel(w - 1, y);
        for dx in 0..amount {
            result.put_pixel(amount + w + dx, y + amount, pixel);
        }
    }

    // Fill corners with the corner pixel
    let corners: [(u32, u32, Rgba<u8>); 4] = [
        (0, 0, *img.get_pixel(0, 0)),                 // top-left
        (w - 1, 0, *img.get_pixel(w - 1, 0)),         // top-right
        (0, h - 1, *img.get_pixel(0, h - 1)),         // bottom-left
        (w - 1, h - 1, *img.get_pixel(w - 1, h - 1)), // bottom-right
    ];

    for (cx, cy, pixel) in &corners {
        let base_x = if *cx == 0 { 0 } else { amount + w };
        let base_y = if *cy == 0 { 0 } else { amount + h };
        for dy in 0..amount {
            for dx in 0..amount {
                result.put_pixel(base_x + dx, base_y + dy, *pixel);
            }
        }
    }

    result
}
