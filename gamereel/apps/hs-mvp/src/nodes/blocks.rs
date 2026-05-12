use image::{DynamicImage, GenericImage, Rgba, RgbaImage};
use super::BLOCK_SIZE;

pub fn gen_block_image(block_dynamic_img: &DynamicImage,
    block_set: [[u32; 3]; 3],
) -> Option<DynamicImage> { 
    let mut block_img =
        RgbaImage::from_pixel(BLOCK_SIZE * 3, BLOCK_SIZE * 3, Rgba([255, 255, 255, 0]));
    // load image
    for (i, row) in block_set.iter().enumerate() {
        for (j, col) in row.iter().enumerate() {
            if *col == 1 {
                let _ = block_img.copy_from(
                    block_dynamic_img,
                    (i as u32) * BLOCK_SIZE,
                    (j as u32) * BLOCK_SIZE,
                );
            }
        }
    };
    let o = DynamicImage::ImageRgba8(block_img);
    Some(o)
}
