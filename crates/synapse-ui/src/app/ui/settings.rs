use gpui_animation::transition::Transition;

use super::super::{
    AppLanguage, SETTINGS_THEME_CONTROL_PADDING, SETTINGS_THEME_CONTROL_WIDTH,
    SETTINGS_THEME_TRANSITION, ThemePreference,
};

pub(in crate::app) fn settings_theme_indicator_left(preference: ThemePreference) -> f32 {
    let segment_width = (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 3.0;
    SETTINGS_THEME_CONTROL_PADDING
        + segment_width
            * match preference {
                ThemePreference::System => 0.0,
                ThemePreference::Light => 1.0,
                ThemePreference::Dark => 2.0,
            }
}

pub(in crate::app) fn settings_language_indicator_left(language: AppLanguage) -> f32 {
    let segment_width = (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 2.0;
    SETTINGS_THEME_CONTROL_PADDING
        + segment_width
            * match language {
                AppLanguage::SimplifiedChinese => 0.0,
                AppLanguage::English => 1.0,
            }
}

pub(in crate::app) fn settings_spring_progress(progress: f32) -> f32 {
    let stiffness = 420.0_f32;
    let damping = 40.0_f32;
    let mass = 0.5_f32;
    let discriminant = (damping * damping - 4.0 * mass * stiffness).sqrt();
    let denominator = 2.0 * mass;
    let slow_root = (-damping + discriminant) / denominator;
    let fast_root = (-damping - discriminant) / denominator;
    let response = |seconds: f32| {
        1.0 + (fast_root * (slow_root * seconds).exp() - slow_root * (fast_root * seconds).exp())
            / (slow_root - fast_root)
    };
    let duration = SETTINGS_THEME_TRANSITION.as_secs_f32();
    (response(progress * duration) / response(duration)).clamp(0.0, 1.0)
}

pub(in crate::app) struct SettingsSpring;

impl Transition for SettingsSpring {
    fn calculate(&self, progress: f32) -> f32 {
        settings_spring_progress(progress)
    }
}
