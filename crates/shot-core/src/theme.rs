//! Design tokens from `docs/design/tokens.md`: the token `overlay.frame-halo` is
//! `OVERLAY_FRAME_HALO` here. Lengths are logical px at 100% scale.

use crate::text::{TypeStyle, Weight};

/// Straight (not premultiplied) sRGB color, written as in the tokens: `0xRRGGBBAA`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn hex(rgba: u32) -> Self {
        let [r, g, b, a] = rgba.to_be_bytes();
        Self { r, g, b, a }
    }
}

pub const OVERLAY_DIM: Color = Color::hex(0x0000_0073);
pub const OVERLAY_FRAME: Color = Color::hex(0xFFFF_FFFF);
pub const OVERLAY_FRAME_HALO: Color = Color::hex(0x0000_0066);
pub const OVERLAY_LABEL_BG: Color = Color::hex(0x1616_1AE0);
pub const OVERLAY_LABEL_TEXT: Color = Color::hex(0xFFFF_FFFF);
pub const OVERLAY_LABEL_TEXT_DIM: Color = Color::hex(0xFFFF_FFA6);
pub const OVERLAY_CROSSHAIR: Color = Color::hex(0xFFFF_FFFF);
pub const OVERLAY_CROSSHAIR_HALO: Color = Color::hex(0x0000_0066);

/// The theme colors of `docs/design/tokens.md`, "Theme colors": one set per theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub bg_canvas: Color,
    pub surface_base: Color,
    pub surface_raised: Color,
    pub surface_sunken: Color,
    pub fill_hover: Color,
    pub fill_pressed: Color,
    pub border_default: Color,
    pub border_strong: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub text_on_accent: Color,
    pub icon_primary: Color,
    pub icon_secondary: Color,
    pub accent_solid: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub accent_subtle: Color,
    pub accent_text: Color,
    pub focus_ring: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub scrim: Color,
}

pub const LIGHT: Palette = Palette {
    dark: false,
    bg_canvas: Color::hex(0xE6E6_EBFF),
    surface_base: Color::hex(0xFFFF_FFFF),
    surface_raised: Color::hex(0xFFFF_FFFF),
    surface_sunken: Color::hex(0xF1F1_F4FF),
    fill_hover: Color::hex(0x1414_190D),
    fill_pressed: Color::hex(0x1414_1917),
    border_default: Color::hex(0x1414_191A),
    border_strong: Color::hex(0x1414_192E),
    text_primary: Color::hex(0x1818_1CFF),
    text_secondary: Color::hex(0x5E5E_69FF),
    text_disabled: Color::hex(0xA2A2_ACFF),
    text_on_accent: Color::hex(0xFFFF_FFFF),
    icon_primary: Color::hex(0x2A2A_30FF),
    icon_secondary: Color::hex(0x6E6E_79FF),
    accent_solid: Color::hex(0x4651_F6FF),
    accent_hover: Color::hex(0x3A44_E0FF),
    accent_pressed: Color::hex(0x3038_C4FF),
    accent_subtle: Color::hex(0x4651_F61F),
    accent_text: Color::hex(0x3A44_E0FF),
    focus_ring: Color::hex(0x4651_F6FF),
    danger: Color::hex(0xC828_1FFF),
    warning: Color::hex(0x9E54_00FF),
    success: Color::hex(0x1780_3CFF),
    scrim: Color::hex(0x0000_004D),
};

pub const DARK: Palette = Palette {
    dark: true,
    bg_canvas: Color::hex(0x1111_14FF),
    surface_base: Color::hex(0x2323_28FF),
    surface_raised: Color::hex(0x2D2D_33FF),
    surface_sunken: Color::hex(0x1A1A_1EFF),
    fill_hover: Color::hex(0xFFFF_FF12),
    fill_pressed: Color::hex(0xFFFF_FF1F),
    border_default: Color::hex(0xFFFF_FF1A),
    border_strong: Color::hex(0xFFFF_FF2E),
    text_primary: Color::hex(0xF2F2_F5FF),
    text_secondary: Color::hex(0xA3A3_AEFF),
    text_disabled: Color::hex(0x5F5F_69FF),
    text_on_accent: Color::hex(0xFFFF_FFFF),
    icon_primary: Color::hex(0xE6E6_EAFF),
    icon_secondary: Color::hex(0x9696_A1FF),
    accent_solid: Color::hex(0x5560_FAFF),
    accent_hover: Color::hex(0x5A64_FFFF),
    accent_pressed: Color::hex(0x4A55_F2FF),
    accent_subtle: Color::hex(0x7A83_FF2E),
    accent_text: Color::hex(0x8F97_FFFF),
    focus_ring: Color::hex(0x7A83_FFFF),
    danger: Color::hex(0xFF62_59FF),
    warning: Color::hex(0xFFB5_47FF),
    success: Color::hex(0x4CD0_7DFF),
    scrim: Color::hex(0x0000_0080),
};

impl Palette {
    pub fn of(dark: bool) -> &'static Palette {
        if dark { &DARK } else { &LIGHT }
    }
}

/// A drop shadow: black, offset down by `y`, blurred with a Gaussian of σ = `blur` / 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub y: f32,
    pub blur: f32,
    pub light: Color,
    pub dark: Color,
}

pub const SHADOW_SM: Shadow = Shadow {
    y: 1.0,
    blur: 3.0,
    light: Color::hex(0x0000_001F),
    dark: Color::hex(0x0000_0059),
};
pub const SHADOW_MD: Shadow = Shadow {
    y: 4.0,
    blur: 12.0,
    light: Color::hex(0x0000_0024),
    dark: Color::hex(0x0000_0066),
};
pub const SHADOW_LG: Shadow = Shadow {
    y: 10.0,
    blur: 30.0,
    light: Color::hex(0x0000_0038),
    dark: Color::hex(0x0000_0080),
};

pub const TYPE_CAPTION: TypeStyle = TypeStyle {
    weight: Weight::Regular,
    size: 11.0,
    line_height: 14.0,
};
pub const TYPE_LABEL: TypeStyle = TypeStyle {
    weight: Weight::SemiBold,
    size: 12.0,
    line_height: 16.0,
};

pub const TYPE_BODY: TypeStyle = TypeStyle {
    weight: Weight::Regular,
    size: 13.0,
    line_height: 18.0,
};
pub const TYPE_BODY_STRONG: TypeStyle = TypeStyle {
    weight: Weight::SemiBold,
    size: 13.0,
    line_height: 18.0,
};
pub const TYPE_TITLE: TypeStyle = TypeStyle {
    weight: Weight::SemiBold,
    size: 15.0,
    line_height: 20.0,
};
pub const TYPE_HEADLINE: TypeStyle = TypeStyle {
    weight: Weight::SemiBold,
    size: 20.0,
    line_height: 26.0,
};

pub const RADIUS_XS: f32 = 4.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;

/// `motion.fade`: the only animation, the preview's and popovers' fade in and out.
pub const FADE_MS: u64 = 120;

/// A length scaled to a monitor: `round(len × s)`.
pub fn scaled(len: f32, scale: f32) -> i32 {
    (len * scale).round() as i32
}

/// A stroke width scaled to a monitor: never thinner than 1 px.
pub fn stroke(width: f32, scale: f32) -> i32 {
    scaled(width, scale).max(1)
}
