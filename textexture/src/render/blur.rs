use tiny_skia::Pixmap;

/// Apply a Gaussian-approximation blur using 3-pass box blur.
/// `radius` is the blur radius in pixels.
pub fn gaussian_blur(pixmap: &mut Pixmap, radius: u32) {
    if radius == 0 {
        return;
    }

    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    let data = pixmap.data_mut();

    // 3-pass box blur approximates Gaussian
    for _ in 0..3 {
        box_blur_h(data, w, h, radius as usize);
        box_blur_v(data, w, h, radius as usize);
    }
}

fn box_blur_h(data: &mut [u8], w: usize, h: usize, radius: usize) {
    let mut row_buf = vec![0u8; w * 4];

    for y in 0..h {
        let row_start = y * w * 4;
        row_buf.copy_from_slice(&data[row_start..row_start + w * 4]);

        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut count = 0u32;

            let start = x.saturating_sub(radius);
            let end = (x + radius + 1).min(w);

            for sx in start..end {
                let idx = sx * 4;
                sum[0] += row_buf[idx] as u32;
                sum[1] += row_buf[idx + 1] as u32;
                sum[2] += row_buf[idx + 2] as u32;
                sum[3] += row_buf[idx + 3] as u32;
                count += 1;
            }

            let idx = row_start + x * 4;
            if count > 0 {
                data[idx] = (sum[0] / count) as u8;
                data[idx + 1] = (sum[1] / count) as u8;
                data[idx + 2] = (sum[2] / count) as u8;
                data[idx + 3] = (sum[3] / count) as u8;
            }
        }
    }
}

fn box_blur_v(data: &mut [u8], w: usize, h: usize, radius: usize) {
    let mut col_buf = vec![0u8; h * 4];

    for x in 0..w {
        // Copy column
        for y in 0..h {
            let idx = (y * w + x) * 4;
            let bidx = y * 4;
            col_buf[bidx..bidx + 4].copy_from_slice(&data[idx..idx + 4]);
        }

        for y in 0..h {
            let mut sum = [0u32; 4];
            let mut count = 0u32;

            let start = y.saturating_sub(radius);
            let end = (y + radius + 1).min(h);

            for sy in start..end {
                let bidx = sy * 4;
                sum[0] += col_buf[bidx] as u32;
                sum[1] += col_buf[bidx + 1] as u32;
                sum[2] += col_buf[bidx + 2] as u32;
                sum[3] += col_buf[bidx + 3] as u32;
                count += 1;
            }

            let idx = (y * w + x) * 4;
            if count > 0 {
                data[idx] = (sum[0] / count) as u8;
                data[idx + 1] = (sum[1] / count) as u8;
                data[idx + 2] = (sum[2] / count) as u8;
                data[idx + 3] = (sum[3] / count) as u8;
            }
        }
    }
}
