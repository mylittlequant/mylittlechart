//! Default signal profiles for common indicators
//!
//! This module provides ready-to-use SignalProfiles for popular indicators.
//! Uses BarIndicatorId for type-safe indicator identification.

use crate::bar_indicators::bar_indicator_id::BarIndicatorId;
use super::config::{DetectorConfig, ValueSource};
use super::profile::SignalProfile;

/// Get default signal profile for an indicator by BarIndicatorId
pub fn default_profile(indicator_id: BarIndicatorId) -> Option<SignalProfile> {
    match indicator_id {
        // Momentum indicators
        BarIndicatorId::Rsi => Some(rsi_profile()),
        BarIndicatorId::Stoch | BarIndicatorId::Stochkd => Some(stochastic_profile()),
        BarIndicatorId::Cci => Some(cci_profile()),
        BarIndicatorId::Mfi => Some(mfi_profile()),
        BarIndicatorId::WilliamsR => Some(williams_r_profile()),
        BarIndicatorId::Cmo => Some(cmo_profile()),
        BarIndicatorId::Roc => Some(roc_profile()),

        // Trend indicators
        BarIndicatorId::Macd => Some(macd_profile()),
        BarIndicatorId::Adx => Some(adx_profile()),
        BarIndicatorId::Aroon => Some(aroon_profile()),
        BarIndicatorId::Supertrend => Some(supertrend_profile()),
        BarIndicatorId::Psar => Some(psar_profile()),

        // Channel indicators
        BarIndicatorId::Bb => Some(bollinger_profile()),
        BarIndicatorId::Kc => Some(keltner_profile()),
        BarIndicatorId::Dc => Some(donchian_profile()),
        BarIndicatorId::Atrchan => Some(atr_channel_profile()),

        // Ichimoku
        BarIndicatorId::Ichimoku => Some(ichimoku_profile()),

        // Volume indicators
        BarIndicatorId::Obv => Some(obv_profile()),
        BarIndicatorId::Vwap => Some(vwap_profile()),
        BarIndicatorId::Cmf => Some(cmf_profile()),
        BarIndicatorId::Ad => Some(ad_profile()),

        // Volatility
        BarIndicatorId::Atr => Some(atr_profile()),

        // Moving averages
        BarIndicatorId::Sma
        | BarIndicatorId::Ema
        | BarIndicatorId::Wma
        | BarIndicatorId::Dema
        | BarIndicatorId::Tema
        | BarIndicatorId::Kama
        | BarIndicatorId::Hma
        | BarIndicatorId::Frama
        | BarIndicatorId::Vidya => Some(ma_profile(indicator_id)),

        // Other indicators
        BarIndicatorId::Tsi => Some(tsi_profile()),
        BarIndicatorId::Ppo => Some(ppo_profile()),
        BarIndicatorId::Ao => Some(ao_profile()),

        _ => None,
    }
}

// ============================================================================
// Momentum Indicators
// ============================================================================

/// RSI default profile - overbought/oversold with divergence
pub fn rsi_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Rsi, "RSI Default")
        .with_description("RSI signals: overbought/oversold levels, centerline cross, divergences")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 70.0, 30.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, 80.0, 20.0),
            DetectorConfig::threshold("centerline", "Centerline Cross", ValueSource::Main, 50.0, 50.0),
            DetectorConfig::divergence("divergence", "RSI Divergence", ValueSource::Main, 14),
            DetectorConfig::swing("swing", "RSI Swings", ValueSource::Main, 5),
        ])
}

/// Stochastic default profile - %K/%D crossovers and overbought/oversold
pub fn stochastic_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Stoch, "Stochastic Default")
        .with_description("Stochastic signals: K/D crossovers, overbought/oversold zones")
        .with_detectors([
            // Stoch returns Double(k, d)
            DetectorConfig::crossover("kd_cross", "%K/%D Crossover", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::First, 80.0, 20.0),
            DetectorConfig::divergence("divergence", "Stochastic Divergence", ValueSource::First, 14),
        ])
}

/// CCI default profile - zero cross and extreme levels
pub fn cci_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Cci, "CCI Default")
        .with_description("CCI signals: zero line cross, overbought/oversold at +/-100")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 5.0),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 100.0, -100.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, 200.0, -200.0),
            DetectorConfig::divergence("divergence", "CCI Divergence", ValueSource::Main, 14),
        ])
}

/// MFI default profile - similar to RSI but volume-weighted
pub fn mfi_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Mfi, "MFI Default")
        .with_description("Money Flow Index: overbought/oversold levels")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 80.0, 20.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, 90.0, 10.0),
            DetectorConfig::divergence("divergence", "MFI Divergence", ValueSource::Main, 14),
        ])
}

/// Williams %R default profile
pub fn williams_r_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::WilliamsR, "Williams %R Default")
        .with_description("Williams %R: overbought/oversold levels (-20/-80)")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, -20.0, -80.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, -10.0, -90.0),
        ])
}

/// CMO (Chande Momentum Oscillator) default profile
pub fn cmo_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Cmo, "CMO Default")
        .with_description("Chande Momentum Oscillator: zero cross and overbought/oversold")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 2.0),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 50.0, -50.0),
        ])
}

/// ROC (Rate of Change) default profile
pub fn roc_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Roc, "ROC Default")
        .with_description("Rate of Change: zero line crossings")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
            DetectorConfig::divergence("divergence", "ROC Divergence", ValueSource::Main, 10),
        ])
}

// ============================================================================
// Trend Indicators
// ============================================================================

/// MACD default profile - comprehensive signals
pub fn macd_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Macd, "MACD Default")
        .with_description("MACD signals: signal line cross, zero line cross, histogram changes")
        .with_detectors([
            DetectorConfig::crossover("signal_cross", "Signal Line Cross", ValueSource::MacdLine, ValueSource::MacdSignal),
            DetectorConfig::zero_cross("zero_cross", "Zero Line Cross", ValueSource::MacdLine, 0.001),
            DetectorConfig::histogram("histogram", "Histogram Direction", ValueSource::MacdHistogram),
            DetectorConfig::divergence("divergence", "MACD Divergence", ValueSource::MacdLine, 14),
        ])
}

/// ADX default profile - trend strength and DI crossovers
pub fn adx_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Adx, "ADX Default")
        .with_description("ADX signals: trend strength levels, DI crossovers")
        .with_detectors([
            // ADX returns Single for main ADX value
            DetectorConfig::threshold("trend_strength", "Trend Strength", ValueSource::Main, 25.0, 20.0),
            DetectorConfig::threshold("strong_trend", "Strong Trend", ValueSource::Main, 40.0, 15.0),
        ])
}

/// Aroon default profile
pub fn aroon_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Aroon, "Aroon Default")
        .with_description("Aroon signals: up/down crossovers, extreme levels")
        .with_detectors([
            // Aroon returns Double(up, down)
            DetectorConfig::crossover("aroon_cross", "Aroon Up/Down Cross", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("strong_up", "Strong Uptrend", ValueSource::First, 70.0, 30.0),
            DetectorConfig::threshold("strong_down", "Strong Downtrend", ValueSource::Second, 70.0, 30.0),
        ])
}

/// SuperTrend default profile
pub fn supertrend_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Supertrend, "SuperTrend Default")
        .with_description("SuperTrend signals: trend direction changes")
        .with_detectors([
            DetectorConfig::zero_cross("direction", "Trend Direction Change", ValueSource::Main, 0.5),
        ])
}

/// Parabolic SAR default profile
pub fn psar_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Psar, "Parabolic SAR Default")
        .with_description("Parabolic SAR: trend reversal signals")
        .with_detectors([
            DetectorConfig::zero_cross("reversal", "SAR Reversal", ValueSource::Main, 0.1),
        ])
}

// ============================================================================
// Channel Indicators
// ============================================================================

/// Bollinger Bands default profile
pub fn bollinger_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Bb, "Bollinger Bands Default")
        .with_description("Bollinger Bands: band touches, squeeze, expansion")
        .with_detectors([
            DetectorConfig::channel("band_touch", "Band Touch", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// Keltner Channel default profile
pub fn keltner_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Kc, "Keltner Channel Default")
        .with_description("Keltner Channel: channel breakouts and returns")
        .with_detectors([
            DetectorConfig::channel("channel", "Channel Position", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// Donchian Channel default profile
pub fn donchian_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Dc, "Donchian Channel Default")
        .with_description("Donchian Channel: breakout signals")
        .with_detectors([
            DetectorConfig::channel("breakout", "Channel Breakout", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// ATR Channel default profile
pub fn atr_channel_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Atrchan, "ATR Channel Default")
        .with_description("ATR-based channel: volatility envelope signals")
        .with_detectors([
            DetectorConfig::channel("channel", "ATR Channel", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

// ============================================================================
// Ichimoku
// ============================================================================

/// Ichimoku Cloud default profile - comprehensive signals
pub fn ichimoku_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Ichimoku, "Ichimoku Default")
        .with_description("Ichimoku: TK cross, price vs cloud, cloud twist")
        .with_detectors([
            DetectorConfig::crossover("tk_cross", "Tenkan/Kijun Cross", ValueSource::IchimokuTenkan, ValueSource::IchimokuKijun),
            DetectorConfig::crossover("cloud_twist", "Cloud Twist (Senkou A/B)", ValueSource::IchimokuSenkouA, ValueSource::IchimokuSenkouB),
        ])
}

// ============================================================================
// Volume Indicators
// ============================================================================

/// OBV default profile
pub fn obv_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Obv, "OBV Default")
        .with_description("On Balance Volume: trend confirmation and divergences")
        .with_detectors([
            DetectorConfig::divergence("divergence", "OBV Divergence", ValueSource::Main, 14),
            DetectorConfig::swing("swing", "OBV Swings", ValueSource::Main, 5),
        ])
}

/// VWAP default profile
pub fn vwap_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Vwap, "VWAP Default")
        .with_description("VWAP: price crossover signals")
        .with_detectors([
            DetectorConfig::swing("swing", "VWAP Swings", ValueSource::Main, 5),
        ])
}

/// CMF default profile
pub fn cmf_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Cmf, "CMF Default")
        .with_description("Chaikin Money Flow: zero line cross and accumulation/distribution")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.01),
            DetectorConfig::threshold("strong", "Strong Flow", ValueSource::Main, 0.25, -0.25),
        ])
}

/// Accumulation/Distribution default profile
pub fn ad_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Ad, "A/D Default")
        .with_description("Accumulation/Distribution: divergence signals")
        .with_detectors([
            DetectorConfig::divergence("divergence", "A/D Divergence", ValueSource::Main, 14),
        ])
}

// ============================================================================
// Volatility Indicators
// ============================================================================

/// ATR default profile
pub fn atr_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Atr, "ATR Default")
        .with_description("Average True Range: volatility expansion/contraction")
        .with_detectors([
            DetectorConfig::swing("volatility", "Volatility Changes", ValueSource::Main, 5),
        ])
}

// ============================================================================
// Moving Averages
// ============================================================================

/// Generic moving average profile
pub fn ma_profile(indicator_id: BarIndicatorId) -> SignalProfile {
    SignalProfile::new(indicator_id, format!("{:?} Default", indicator_id))
        .with_description("Moving average: price crossover signals")
        .with_detectors([
            DetectorConfig::price_crossover("crossover", "Price Crossover MA"),
        ])
}

// ============================================================================
// Other Oscillators
// ============================================================================

/// TSI default profile
pub fn tsi_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Tsi, "TSI Default")
        .with_description("True Strength Index: zero cross and signal line")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 2.0),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 25.0, -25.0),
        ])
}

/// PPO default profile
pub fn ppo_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Ppo, "PPO Default")
        .with_description("Percentage Price Oscillator: zero cross")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
        ])
}

/// Awesome Oscillator default profile
pub fn ao_profile() -> SignalProfile {
    SignalProfile::new(BarIndicatorId::Ao, "AO Default")
        .with_description("Awesome Oscillator: zero cross and saucer signals")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
            DetectorConfig::histogram("histogram", "AO Histogram", ValueSource::Main),
        ])
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_profile_exists() {
        // Test that common indicators have default profiles
        assert!(default_profile(BarIndicatorId::Rsi).is_some());
        assert!(default_profile(BarIndicatorId::Macd).is_some());
        assert!(default_profile(BarIndicatorId::Bb).is_some());
        assert!(default_profile(BarIndicatorId::Adx).is_some());
        assert!(default_profile(BarIndicatorId::Stoch).is_some());
        assert!(default_profile(BarIndicatorId::Ichimoku).is_some());
    }

    #[test]
    fn test_unknown_indicator_returns_none() {
        // Pick an indicator without a profile
        assert!(default_profile(BarIndicatorId::Autocorr).is_none());
    }

    #[test]
    fn test_rsi_profile_has_expected_detectors() {
        let profile = rsi_profile();
        assert!(profile.get_detector("ob_os").is_some());
        assert!(profile.get_detector("extreme").is_some());
        assert!(profile.get_detector("divergence").is_some());
    }

    #[test]
    fn test_macd_profile_has_expected_detectors() {
        let profile = macd_profile();
        assert!(profile.get_detector("signal_cross").is_some());
        assert!(profile.get_detector("zero_cross").is_some());
        assert!(profile.get_detector("histogram").is_some());
        assert!(profile.get_detector("divergence").is_some());
    }

    #[test]
    fn test_profile_customization() {
        let mut profile = rsi_profile();

        // Customize thresholds
        profile.update_threshold("ob_os", 80.0, 20.0);

        let detector = profile.get_detector("ob_os").unwrap();
        if let super::super::config::DetectorParams::Threshold { upper, lower, .. } =
            &detector.params
        {
            assert_eq!(*upper, 80.0);
            assert_eq!(*lower, 20.0);
        }
    }

    #[test]
    fn test_ma_variants() {
        // All MA variants should return a profile
        for id in [
            BarIndicatorId::Sma,
            BarIndicatorId::Ema,
            BarIndicatorId::Wma,
            BarIndicatorId::Dema,
            BarIndicatorId::Tema,
        ] {
            assert!(default_profile(id).is_some(), "Missing profile for {:?}", id);
        }
    }
}
