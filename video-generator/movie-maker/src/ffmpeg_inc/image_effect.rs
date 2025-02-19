use image::{DynamicImage, GenericImage, GenericImageView, ImageBuffer, Rgba};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
use image::imageops::resize;

pub fn blend_images(
    base: &mut DynamicImage,
    overlay_img: &DynamicImage,
    x: f32,
    y: f32,
    width: Option<f32>,    // 可选宽度
    height: Option<f32>,   // 可选高度
    scale_x: Option<f32>,  // 可选缩放比例（宽）
    scale_y: Option<f32>,  // 可选缩放比例（高）
    rotation: Option<f32>, // 可选旋转角度（单位：度）
    opacity: Option<u32>,  // 可选透明度（0-255）
    anchor_x: Option<f32>, // 可选锚点 x 坐标（0.0-1.0）
    anchor_y: Option<f32>, // 可选锚点 y 坐标（0.0-1.0）
) {
    let (overlay_width, overlay_height) = overlay_img.dimensions();

    // 确定宽度和高度：使用提供的值或默认值
    let scale_x = scale_x.unwrap_or(1.0); // 默认比例为 1.0
    let scale_y = scale_y.unwrap_or(1.0); // 默认比例为 1.0
    let target_width = width.unwrap_or(overlay_width as f32) * scale_x;
    let target_height = height.unwrap_or(overlay_height as f32) * scale_y;

    // 确定旋转角度：使用提供的值或默认值
    let rotation_angle = rotation.unwrap_or(0.0);

    // 确定锚点：使用提供的值或默认值
    let anchor_x = anchor_x.unwrap_or(0.0); // 默认锚点为图像的中心
    let anchor_y = anchor_y.unwrap_or(0.0); // 默认锚点为图像的中心

    // 缩放叠加图像到指定宽高
    let resized_overlay = resize(
        overlay_img,
        target_width as u32,
        target_height as u32,
        image::imageops::FilterType::Triangle,
    );

    // 将 resized_overlay 转换为 DynamicImage
    let resized_overlay_dynamic = DynamicImage::ImageRgba8(resized_overlay);

    // 应用透明度（如果有的话）
    let overlay_with_opacity = apply_opacity(&resized_overlay_dynamic, opacity.unwrap_or(255));

    // 扩展画布以适应旋转后的图像
    let expanded_canvas = expand_canvas(&overlay_with_opacity, rotation_angle);

    // 旋转扩展后的叠加图像
    let rotated_overlay = rotate_about_center(
        &expanded_canvas.to_rgba8(),
        rotation_angle.to_radians(),
        Interpolation::Bilinear,
        Rgba([0, 0, 0, 0]), // 填充透明区域
    );

    let rotated_overlay = DynamicImage::ImageRgba8(rotated_overlay);

    // 根据锚点调整叠加图像的起始位置
    let (adjusted_x, adjusted_y) = calculate_anchor_offset(
        anchor_x, anchor_y,
        target_width as u32, target_height as u32,
        base.dimensions(),
        rotation_angle,
        x, y,
    );

    // 叠加图像
    blend_images_internal(base, &rotated_overlay, adjusted_x, adjusted_y);
}

// 计算锚点偏移量
fn calculate_anchor_offset(
    anchor_x: f32,
    anchor_y: f32,
    overlay_width: u32,
    overlay_height: u32,
    base_dim: (u32, u32),
    rotation_angle:f32,
    x: f32,
    y: f32,
) -> (f32, f32) {
    let (base_width, base_height) = base_dim;

    // 计算锚点相对于图像左上角的偏移
    let anchor_offset_x = anchor_x * overlay_width as f32 - overlay_width as f32 * 0.5;
    let anchor_offset_y = anchor_y * overlay_height as f32 - overlay_height as f32 * 0.5;


    // 锚点旋转前的图像左上角坐标（相对于目标位置 x, y）
    let unrotated_x = x - anchor_offset_x - overlay_width as f32 * 0.5;
    let unrotated_y = y - anchor_offset_y - overlay_height as f32 * 0.5;

    // TODO rouation 和 锚点同时生效 有问题
    // // println!("rotated_x = {}, unrotated_y = {:?}", unrotated_x, unrotated_y);
    // // 将角度转换为弧度
    // let angle_rad = rotation_angle * PI / 180.0;

    // // 计算旋转后的偏移量
    // let cos_theta = angle_rad.cos();
    // let sin_theta = angle_rad.sin();
    // // println!("angle_rad = {}, angle_rad = {:?}", angle_rad, angle_rad);
    // // println!("cos_theta = {}, sin_theta = {:?}", cos_theta, sin_theta);

    // // 绕锚点旋转后的位置（计算相对于目标位置 x, y 的新左上角坐标）
    // let rotated_x = overlay_width as f32 * cos_theta * (0.5-anchor_x) - overlay_height as f32 * sin_theta * (0.5-anchor_y) + unrotated_x;
    // let rotated_y = overlay_width as f32 * sin_theta * (0.5-anchor_x) + overlay_height as f32 * cos_theta * (0.5-anchor_y) + unrotated_y;
    // // println!("rotated_x = {}, rotated_y = {:?}", rotated_x, rotated_y);
    // (rotated_x, rotated_y)
    return (unrotated_x, unrotated_y)
}

// 扩展画布，确保旋转后图像不裁剪
fn expand_canvas(image: &DynamicImage, rotation: f32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let angle_rad = rotation.to_radians();

    // 计算旋转后图像的外接矩形尺寸
    let new_width = (width as f32 * angle_rad.cos().abs() + height as f32 * angle_rad.sin().abs()).ceil() as u32;
    let new_height = (width as f32 * angle_rad.sin().abs() + height as f32 * angle_rad.cos().abs()).ceil() as u32;

    // 创建一个新的画布
    let mut canvas = ImageBuffer::from_pixel(new_width, new_height, Rgba([0, 0, 0, 0]));
    let offset_x = (new_width - width) / 2;
    let offset_y = (new_height - height) / 2;

    // 将原始图像放置在画布中心
    for y in 0..height {
        for x in 0..width {
            let pixel = image.get_pixel(x, y);
            canvas.put_pixel(x + offset_x, y + offset_y, pixel);
        }
    }

    DynamicImage::ImageRgba8(canvas)
}

// 应用透明度到图像
fn apply_opacity(overlay_img: &DynamicImage, opacity: u32) -> DynamicImage {
    let mut image = overlay_img.to_rgba8(); // 获取图像的 RGBA 数据
    
    // 确保 opacity 在合法范围内
    let opacity = opacity.clamp(0, 255);
    
    for pixel in image.pixels_mut() {
        // 获取原始图像的透明度（alpha 通道）
        let original_alpha = pixel[3] as f32;
        
        // 计算新的透明度（根据提供的 opacity 调整）
        let new_alpha = (original_alpha * opacity as f32) / 255.0;
        
        // 确保新的透明度在合法范围内 [0, 255]
        pixel[3] = new_alpha.clamp(0.0, 255.0) as u8;
    }
    
    DynamicImage::ImageRgba8(image)
}

// 原始 blend_images 内部逻辑提取到 blend_images_internal
fn blend_images_internal(
    base: &mut DynamicImage,
    overlay_img: &DynamicImage,
    x: f32,
    y: f32,
) {
    let (base_width, base_height) = base.dimensions();
    let (overlay_width, overlay_height) = overlay_img.dimensions();

    // 浮点坐标转换为整数
    let x_start = x.floor() as i32;
    let y_start = y.floor() as i32;

    // 计算有效范围：确保叠加图像的像素在 base 范围内
    let x_overlay_start = if x_start < 0 { -x_start } else { 0 } as u32;
    let y_overlay_start = if y_start < 0 { -y_start } else { 0 } as u32;

    let x_base_start = x_start.max(0) as u32;
    let y_base_start = y_start.max(0) as u32;

    let x_overlay_end = overlay_width.min(base_width.saturating_sub(x_base_start));
    let y_overlay_end = overlay_height.min(base_height.saturating_sub(y_base_start));

    for oy in y_overlay_start..y_overlay_end {
        for ox in x_overlay_start..x_overlay_end {
            let px = x_base_start + (ox - x_overlay_start);
            let py = y_base_start + (oy - y_overlay_start);

            let base_pixel = base.get_pixel(px, py);
            let overlay_pixel = overlay_img.get_pixel(ox, oy);

            // 透明度混合
            let blended_pixel = blend_pixel(base_pixel, overlay_pixel);
            base.put_pixel(px, py, blended_pixel);
        }
    }
}

// 混合两个像素（带透明度）
fn blend_pixel(base: Rgba<u8>, overlay: Rgba<u8>) -> Rgba<u8> {
    let alpha_overlay = overlay[3] as f32 / 255.0;
    let alpha_base = base[3] as f32 / 255.0;
    let alpha_composite = alpha_overlay + alpha_base * (1.0 - alpha_overlay);

    if alpha_composite > 0.0 {
        let r = (overlay[0] as f32 * alpha_overlay + base[0] as f32 * alpha_base * (1.0 - alpha_overlay)) / alpha_composite;
        let g = (overlay[1] as f32 * alpha_overlay + base[1] as f32 * alpha_base * (1.0 - alpha_overlay)) / alpha_composite;
        let b = (overlay[2] as f32 * alpha_overlay + base[2] as f32 * alpha_base * (1.0 - alpha_overlay)) / alpha_composite;
        let a = alpha_composite * 255.0;

        Rgba([r as u8, g as u8, b as u8, a as u8])
    } else {
        base
    }
}
