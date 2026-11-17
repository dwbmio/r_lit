//! Spine 紧凑窗口布局：单一数据源 + 启动/单元测试校验，避免 UI 区互相重叠。

/// 变更布局时递增，便于日志确认已部署新版本。
pub const LAYOUT_VERSION: &str = "spine-layout-v5";

pub const WIN_W: f32 = 198.0;
pub const WIN_H: f32 = 184.0;
pub const PET_COL_W: f32 = 112.0;
pub const HUD_COL_W: f32 = 86.0;

/// 左上待办（仅占用宠物列，不跨入 HUD 列）。
pub const NOTIFY: UiRect = UiRect::new(6.0, 6.0, 90.0, 58.0);
/// 右侧 ⚙（HUD 列顶部，与待办水平分离 ≥14px）。
pub const GEAR: UiRect = UiRect::new(120.0, 8.0, 26.0, 26.0);
/// 展开动画面板。
pub const HUD_PANEL: UiRect = UiRect::new(PET_COL_W, 0.0, HUD_COL_W, WIN_H);
/// Spine 相机视口（待办下方留白 ≥16px）。
pub const PET_VIEWPORT: UiRect = UiRect::new(0.0, 80.0, PET_COL_W, WIN_H - 80.0);

pub const MIN_NOTIFY_PET_GAP: f32 = 16.0;
pub const MIN_NOTIFY_GEAR_GAP_X: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    pub fn overlaps(self, o: Self) -> bool {
        self.x < o.right() && self.right() > o.x && self.y < o.bottom() && self.bottom() > o.y
    }

    pub fn gap_x(self, o: Self) -> f32 {
        if self.right() <= o.x {
            o.x - self.right()
        } else if o.right() <= self.x {
            self.x - o.right()
        } else {
            -1.0
        }
    }

    pub fn gap_y_below(self, upper: Self) -> f32 {
        self.y - upper.bottom()
    }
}

pub fn validate_layout() -> Result<(), String> {
    if NOTIFY.right() > PET_COL_W {
        return Err(format!(
            "notify 跨入 HUD 列: right={} > PET_COL_W={}",
            NOTIFY.right(),
            PET_COL_W
        ));
    }
    if GEAR.x < PET_COL_W {
        return Err(format!("gear 在宠物列: x={}", GEAR.x));
    }
    if NOTIFY.overlaps(GEAR) {
        return Err("notify 与 gear 重叠".into());
    }
    let gx = NOTIFY.gap_x(GEAR);
    if gx < MIN_NOTIFY_GEAR_GAP_X {
        return Err(format!("notify-gear 水平间距 {gx}px < {MIN_NOTIFY_GEAR_GAP_X}px"));
    }
    let gy = PET_VIEWPORT.gap_y_below(NOTIFY);
    if gy < MIN_NOTIFY_PET_GAP {
        return Err(format!("notify-宠物 垂直间距 {gy}px < {MIN_NOTIFY_PET_GAP}px"));
    }
    if PET_VIEWPORT.right() > PET_COL_W + 0.5 {
        return Err("宠物视口超出宠物列".into());
    }
    if (WIN_W - PET_COL_W - HUD_COL_W).abs() > 0.5 {
        return Err(format!(
            "窗口列宽不一致: WIN_W={WIN_W} PET+HUD={}",
            PET_COL_W + HUD_COL_W
        ));
    }
    Ok(())
}

pub fn layout_log_line() -> String {
    format!(
        "{LAYOUT_VERSION} OK | notify=({:.0},{:.0},{:.0},{:.0}) gear=({:.0},{:.0}) pet_vp_y={:.0} gap_notify_pet={:.0}px gap_notify_gear={:.0}px",
        NOTIFY.x,
        NOTIFY.y,
        NOTIFY.w,
        NOTIFY.h,
        GEAR.x,
        GEAR.y,
        PET_VIEWPORT.y,
        PET_VIEWPORT.gap_y_below(NOTIFY),
        NOTIFY.gap_x(GEAR),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_has_no_overlaps() {
        validate_layout().expect("layout invalid");
    }

    #[test]
    fn notify_stays_left_of_hud_divider() {
        assert!(NOTIFY.right() <= PET_COL_W);
        assert!(GEAR.x >= PET_COL_W + MIN_NOTIFY_GEAR_GAP_X - GEAR.w);
    }

    #[test]
    fn pet_starts_below_notify_with_gap() {
        assert!(PET_VIEWPORT.gap_y_below(NOTIFY) >= MIN_NOTIFY_PET_GAP);
    }
}
