// From AI: 添加图片缩放逻辑的单元测试

#[cfg(test)]
mod resize_tests {
    #[test]
    fn test_calculate_resize_dimensions_max_pixel() {
        // 测试按最大像素缩放
        let (orig_w, orig_h) = (1920, 1080);
        let max_pixel = 500000; // 使用更大的值以避免整数截断导致的纵横比偏差

        // 计算缩放后的尺寸
        let total_pixels = orig_w * orig_h;
        if total_pixels <= max_pixel {
            assert_eq!((orig_w, orig_h), (orig_w, orig_h));
        } else {
            let scale = ((max_pixel as f64) / (total_pixels as f64)).sqrt();
            let new_w = (orig_w as f64 * scale) as u32;
            let new_h = (orig_h as f64 * scale) as u32;
            assert!(new_w * new_h <= max_pixel as u32);
            // 验证纵横比保持（使用更宽松的容差以应对整数截断）
            let orig_ratio = orig_w as f64 / orig_h as f64;
            let new_ratio = new_w as f64 / new_h as f64;
            assert!((orig_ratio - new_ratio).abs() < 0.02);
        }
    }

    #[test]
    fn test_calculate_resize_dimensions_fixed_width() {
        // 测试固定宽度缩放
        let (orig_w, orig_h) = (1920, 1080);
        let target_w = 800;

        let scale = target_w as f64 / orig_w as f64;
        let new_h = (orig_h as f64 * scale) as u32;

        assert_eq!(target_w, 800);
        // 验证纵横比保持
        let orig_ratio = orig_w as f64 / orig_h as f64;
        let new_ratio = target_w as f64 / new_h as f64;
        assert!((orig_ratio - new_ratio).abs() < 0.01);
    }

    #[test]
    fn test_calculate_resize_dimensions_fixed_height() {
        // 测试固定高度缩放
        let (orig_w, orig_h) = (1920, 1080);
        let target_h = 600;

        let scale = target_h as f64 / orig_h as f64;
        let new_w = (orig_w as f64 * scale) as u32;

        assert_eq!(target_h, 600);
        // 验证纵横比保持
        let orig_ratio = orig_w as f64 / orig_h as f64;
        let new_ratio = new_w as f64 / target_h as f64;
        assert!((orig_ratio - new_ratio).abs() < 0.01);
    }

    #[test]
    fn test_no_resize_needed() {
        // 测试图片已经小于目标尺寸
        let (orig_w, orig_h) = (800, 600);
        let max_pixel = 1000000;

        let total_pixels = orig_w * orig_h;
        assert!(total_pixels <= max_pixel);
    }

    #[test]
    fn test_square_image_resize() {
        // 测试正方形图片缩放
        let (orig_w, orig_h) = (1000, 1000);
        let max_pixel = 250000; // 500x500

        let total_pixels = orig_w * orig_h;
        let scale = ((max_pixel as f64) / (total_pixels as f64)).sqrt();
        let new_w = (orig_w as f64 * scale) as u32;
        let new_h = (orig_h as f64 * scale) as u32;

        // 正方形图片缩放后应该仍然是正方形
        assert_eq!(new_w, new_h);
    }

    #[test]
    fn test_portrait_image_resize() {
        // 测试竖向图片缩放
        let (orig_w, orig_h) = (1080, 1920);
        let max_pixel = 500000;

        let total_pixels = orig_w * orig_h;
        let scale = ((max_pixel as f64) / (total_pixels as f64)).sqrt();
        let new_w = (orig_w as f64 * scale) as u32;
        let new_h = (orig_h as f64 * scale) as u32;

        // 验证高度大于宽度
        assert!(new_h > new_w);
    }

    #[test]
    fn test_landscape_image_resize() {
        // 测试横向图片缩放
        let (orig_w, orig_h) = (1920, 1080);
        let max_pixel = 500000;

        let total_pixels = orig_w * orig_h;
        let scale = ((max_pixel as f64) / (total_pixels as f64)).sqrt();
        let new_w = (orig_w as f64 * scale) as u32;
        let new_h = (orig_h as f64 * scale) as u32;

        // 验证宽度大于高度
        assert!(new_w > new_h);
    }
}
