//! 品牌设计 tokens(亮色基底)— 对应设计方案 §3.2 / §4.1
#![allow(dead_code)]

use eframe::egui::Color32;

/// 页面浅灰
pub const BG_PAGE: Color32 = Color32::from_rgb(0xF6, 0xF8, 0xFB);
/// 侧栏 / 右栏面板
pub const PANEL: Color32 = Color32::from_rgb(0xF8, 0xFA, 0xFC);
/// 白面板
pub const WHITE: Color32 = Color32::WHITE;
/// 描边
pub const BORDER: Color32 = Color32::from_rgb(0xE4, 0xEA, 0xF1);

pub const TEXT: Color32 = Color32::from_rgb(0x0F, 0x17, 0x2A);
pub const TEXT2: Color32 = Color32::from_rgb(0x33, 0x41, 0x55);
pub const MUTED: Color32 = Color32::from_rgb(0x64, 0x74, 0x8B);
pub const FAINT: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);

/// 天蓝 = 主色 / 交互 / 进行中
pub const SKY: Color32 = Color32::from_rgb(0x0E, 0xA5, 0xE9);
pub const SKY_DEEP: Color32 = Color32::from_rgb(0x02, 0x84, 0xC7);
pub const SKY_CHIP_BG: Color32 = Color32::from_rgb(0xF0, 0xF9, 0xFF);
pub const SKY_CHIP_BR: Color32 = Color32::from_rgb(0xBA, 0xE6, 0xFD);
pub const SKY_CHIP_TX: Color32 = Color32::from_rgb(0x03, 0x69, 0xA1);
pub const SKY_BUBBLE_TX: Color32 = Color32::from_rgb(0x0C, 0x4A, 0x6E);

/// 浅绿 = 强调 / 成功 / 产物
pub const GRN: Color32 = Color32::from_rgb(0x10, 0xB9, 0x81);
pub const GRN_CHIP_BG: Color32 = Color32::from_rgb(0xEC, 0xFD, 0xF5);
pub const GRN_CHIP_BR: Color32 = Color32::from_rgb(0xA7, 0xF3, 0xD0);
pub const GRN_CHIP_TX: Color32 = Color32::from_rgb(0x04, 0x78, 0x57);

/// diff / 代码
pub const DEL_TX: Color32 = Color32::from_rgb(0xB9, 0x1C, 0x1C);
pub const DEL_BG: Color32 = Color32::from_rgb(0xFE, 0xF2, 0xF2);
pub const CODE_BG: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);

/// 交互表面(会话选中/悬停,中性灰对齐参考形态)
pub const HOVER: Color32 = Color32::from_rgb(0xF5, 0xF7, 0xFA);
pub const SELECTED: Color32 = Color32::from_rgb(0xEE, 0xF2, 0xF7);

/// 用户气泡
pub const BUBBLE_BG: Color32 = Color32::from_rgb(0xEA, 0xF2, 0xFE);

/// 文本选中背景(校准蓝色,正文保持黑色可读)
pub const SELECTION_BG: Color32 = Color32::from_rgb(0xBF, 0xDB, 0xFE);

/// 对话正文字体(略粗的无衬线,学 ZCode 观感)
pub fn serif(size: f32) -> eframe::egui::FontId {
    eframe::egui::FontId::proportional(size)
}

/// 粗体(无衬线)字体
pub fn bold(size: f32) -> eframe::egui::FontId {
    eframe::egui::FontId::new(size, eframe::egui::FontFamily::Name("o1bold".into()))
}
