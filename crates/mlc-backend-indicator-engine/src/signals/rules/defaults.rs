//! Default signal profiles for common indicators
//!
//! This module provides ready-to-use SignalProfiles for popular indicators.
//! Uses IndicatorId for type-safe indicator identification.
//!
// removed: LaguerreRsi, DivStrength, MaCross, AutoFibo, SwingsSoft, MtfMomDiv, Rc2, Rc3, Rc4,
// Fibochan, Darkcloudcover, Doji, Engulfing, Eveningstar, Hammer, Harami, Marubozu, Morningstar,
// Patternrec, Piercingpattern, Sfp, Shootingstar, Threeblackcrows, Threewhitesoldiers, Tweezer,
// CandlePatterns, Trin, CciDiv, ClassicDiv, HiddenDiv, MacdDiv, MacdHistDiv, MultiDiv, ObvDiv,
// RsiDiv, StochDiv, VolDiv, WilliamsDiv, ZigzagDiv, Zigzag, ZigzagAtr, ZigzagCandle,
// ZigzagClassic, ZigzagLookahead, ZigzagTime, Swings

use crate::IndicatorId;
use super::config::{DetectorConfig, ValueSource};
use super::profile::SignalProfile;

// ============================================================================
// Template helpers
// ============================================================================

fn bounded_oscillator(id: IndicatorId, name: &str, upper: f64, lower: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: overbought/oversold levels", name))
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, upper, lower),
            DetectorConfig::divergence("divergence", format!("{} Divergence", name), ValueSource::Main, 14),
        ])
}

fn zero_cross_oscillator(id: IndicatorId, name: &str, tolerance: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: zero line crossings", name))
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, tolerance),
            DetectorConfig::divergence("divergence", format!("{} Divergence", name), ValueSource::Main, 14),
        ])
}

fn overlay_crossover(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: price crossover signals", name))
        .with_detectors([
            DetectorConfig::price_crossover("crossover", format!("Price Cross {}", name)),
        ])
}

fn channel_indicator(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: channel breakouts", name))
        .with_detectors([
            DetectorConfig::channel("channel", "Channel Position", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

fn histogram_signal(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: pattern signals", name))
        .with_detectors([
            DetectorConfig::histogram("signal", format!("{} Signal", name), ValueSource::Main),
        ])
}

fn volatility_swing(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: volatility changes", name))
        .with_detectors([
            DetectorConfig::swing("swing", format!("{} Swings", name), ValueSource::Main, 5),
        ])
}

fn swing_levels(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: swing highs/lows", name))
        .with_detectors([
            DetectorConfig::swing("swing", format!("{} Swings", name), ValueSource::Main, 5),
        ])
}

fn zero_cross_threshold(id: IndicatorId, name: &str, tolerance: f64, upper: f64, lower: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: zero cross and extreme levels", name))
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, tolerance),
            DetectorConfig::threshold("levels", "Extreme Levels", ValueSource::Main, upper, lower),
        ])
}

fn dual_line_crossover(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: line crossovers", name))
        .with_detectors([
            DetectorConfig::crossover("cross", format!("{} Crossover", name), ValueSource::First, ValueSource::Second),
        ])
}

fn dual_crossover_threshold(id: IndicatorId, name: &str, upper: f64, lower: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: crossover and overbought/oversold", name))
        .with_detectors([
            DetectorConfig::crossover("cross", format!("{} Crossover", name), ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::First, upper, lower),
        ])
}

fn dual_crossover_zerocross(id: IndicatorId, name: &str, tolerance: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: crossover and zero cross", name))
        .with_detectors([
            DetectorConfig::crossover("cross", format!("{} Crossover", name), ValueSource::First, ValueSource::Second),
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::First, tolerance),
        ])
}

fn threshold_only(id: IndicatorId, name: &str, upper: f64, lower: f64) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: threshold levels", name))
        .with_detectors([
            DetectorConfig::threshold("levels", "Signal Levels", ValueSource::Main, upper, lower),
        ])
}

// ============================================================================
// Main dispatch
// ============================================================================

/// Get default signal profile for an indicator by IndicatorId
pub fn default_profile(indicator_id: IndicatorId) -> Option<SignalProfile> {
    match indicator_id {

        // ============================================================
        // Average / Moving Averages
        // ============================================================
        IndicatorId::Sma
        | IndicatorId::Ema
        | IndicatorId::Wma
        | IndicatorId::Dema
        | IndicatorId::Tema
        | IndicatorId::Kama
        | IndicatorId::Hma
        | IndicatorId::Frama
        | IndicatorId::Vidya
        | IndicatorId::Alma
        | IndicatorId::Ama
        | IndicatorId::Ehlersfa
        | IndicatorId::Ehlersz
        | IndicatorId::Jma
        | IndicatorId::Lr
        | IndicatorId::Mcginley
        | IndicatorId::Rma
        | IndicatorId::T3
        | IndicatorId::Tma
        | IndicatorId::Trima
        | IndicatorId::Vwma => Some(ma_profile(indicator_id)),

        // Vwap is special — stays as swing profile
        IndicatorId::Vwap => Some(vwap_profile()),

        // ============================================================
        // Momentum — Bounded Oscillators (0–100 or 0–1)
        // ============================================================
        IndicatorId::Rsi => Some(rsi_profile()),
        IndicatorId::Stoch | IndicatorId::Stochkd => Some(stochastic_profile()),
        IndicatorId::StochRsi => Some(stoch_rsi_profile()),
        IndicatorId::ConnorsRsi => Some(bounded_oscillator(indicator_id, "ConnorsRSI", 70.0, 30.0)),
        IndicatorId::AdaptiveStoch => Some(dual_crossover_threshold(indicator_id, "Adaptive Stochastic", 80.0, 20.0)),
        IndicatorId::Uo => Some(bounded_oscillator(indicator_id, "Ultimate Oscillator", 70.0, 30.0)),
        IndicatorId::UoSmooth => Some(threshold_only(indicator_id, "UO Smooth", 70.0, 30.0)),
        IndicatorId::Rmi => Some(bounded_oscillator(indicator_id, "RMI", 70.0, 30.0)),
        IndicatorId::Rsioma => Some(bounded_oscillator(indicator_id, "RSIOMA", 70.0, 30.0)),
        IndicatorId::Rsx => Some(bounded_oscillator(indicator_id, "RSX", 70.0, 30.0)),
        IndicatorId::AtrRsi => Some(bounded_oscillator(indicator_id, "ATR RSI", 70.0, 30.0)),
        IndicatorId::IftRsi => Some(zero_cross_threshold(indicator_id, "IFT RSI", 0.05, 0.5, -0.5)),
        IndicatorId::Imi => Some(threshold_only(indicator_id, "IMI", 70.0, 30.0)),
        IndicatorId::Dss => Some(threshold_only(indicator_id, "DSS", 80.0, 20.0)),
        IndicatorId::Kdj => Some(dual_crossover_threshold(indicator_id, "KDJ", 80.0, 20.0)),
        IndicatorId::Tdi => Some(dual_crossover_threshold(indicator_id, "TDI", 68.0, 32.0)),
        IndicatorId::Psl => Some(threshold_only(indicator_id, "Psychological Line", 75.0, 25.0)),
        IndicatorId::Rvgi => Some(dual_crossover_zerocross(indicator_id, "RVGI", 0.001)),
        IndicatorId::Vortex => Some(dual_crossover_threshold(indicator_id, "Vortex", 1.1, 0.9)),
        IndicatorId::Demarker => Some(threshold_only(indicator_id, "DeMarker", 0.7, 0.3)),
        IndicatorId::Vwrsi => Some(bounded_oscillator(indicator_id, "VWRSI", 70.0, 30.0)),
        IndicatorId::Stc => Some(threshold_only(indicator_id, "STC", 75.0, 25.0)),
        IndicatorId::Tii => Some(threshold_only(indicator_id, "TII", 80.0, 20.0)),
        IndicatorId::RelTrendPos => Some(threshold_only(indicator_id, "Rel Trend Pos", 70.0, 30.0)),
        IndicatorId::C2cvp => Some(threshold_only(indicator_id, "C2C Vol Percentile", 80.0, 20.0)),
        IndicatorId::Vovp => Some(threshold_only(indicator_id, "Vol of Vol Percentile", 80.0, 20.0)),
        IndicatorId::DfaPct => Some(threshold_only(indicator_id, "DFA Percent", 60.0, 40.0)),
        IndicatorId::HurstPct => Some(threshold_only(indicator_id, "Hurst Percent", 60.0, 40.0)),
        IndicatorId::RsiPctRank => Some(threshold_only(indicator_id, "RSI Pct Rank", 80.0, 20.0)),
        IndicatorId::DayWeekMonth => Some(threshold_only(indicator_id, "Day/Week/Month", 90.0, 10.0)),
        IndicatorId::Screstp => Some(threshold_only(indicator_id, "Spectral Crest Pct", 80.0, 20.0)),
        IndicatorId::Sflatp => Some(threshold_only(indicator_id, "Spectral Flatness Pct", 70.0, 30.0)),
        IndicatorId::Sslopep => Some(zero_cross_threshold(indicator_id, "Spectral Slope Pct", 2.0, 20.0, -20.0)),
        IndicatorId::Hlva => Some(swing_levels(indicator_id, "HLVA")),
        IndicatorId::Cci => Some(cci_profile()),
        IndicatorId::Mfi => Some(mfi_profile()),
        IndicatorId::WilliamsR => Some(williams_r_profile()),
        IndicatorId::Cmo => Some(cmo_profile()),
        IndicatorId::Roc => Some(roc_profile()),

        // ============================================================
        // Momentum — Centered Oscillators (around zero)
        // ============================================================
        IndicatorId::Macd => Some(macd_profile()),
        IndicatorId::Adx => Some(adx_profile()),
        IndicatorId::Aroon => Some(aroon_profile()),
        IndicatorId::Supertrend => Some(supertrend_profile()),
        IndicatorId::Psar => Some(psar_profile()),
        IndicatorId::Tsi => Some(tsi_profile()),
        IndicatorId::Ppo => Some(ppo_profile()),
        IndicatorId::Ao => Some(ao_profile()),
        IndicatorId::Apo => Some(zero_cross_oscillator(indicator_id, "APO", 0.01)),
        IndicatorId::Bias => Some(zero_cross_oscillator(indicator_id, "Bias", 0.001)),
        IndicatorId::Bop => Some(zero_cross_threshold(indicator_id, "Balance of Power", 0.05, 0.5, -0.5)),
        IndicatorId::Cfo => Some(zero_cross_oscillator(indicator_id, "CFO", 0.1)),
        IndicatorId::Dpo => Some(zero_cross_oscillator(indicator_id, "DPO", 0.1)),
        IndicatorId::DpoPct => Some(zero_cross_oscillator(indicator_id, "DPO Pct", 0.1)),
        IndicatorId::Trix => Some(zero_cross_oscillator(indicator_id, "TRIX", 0.00001)),
        IndicatorId::Coppock => Some(zero_cross_oscillator(indicator_id, "Coppock", 0.1)),
        IndicatorId::Gator => Some(volatility_swing(indicator_id, "Gator")),
        IndicatorId::Qqe => Some(dual_crossover_zerocross(indicator_id, "QQE", 1.0)),
        IndicatorId::Pmo => Some(dual_crossover_zerocross(indicator_id, "PMO", 0.5)),
        IndicatorId::Kst => Some(dual_crossover_zerocross(indicator_id, "KST", 1.0)),
        IndicatorId::ElderRay => Some(elder_ray_profile()),
        IndicatorId::EhlersCc => Some(zero_cross_oscillator(indicator_id, "Ehlers CC", 0.01)),
        IndicatorId::Dsp => Some(zero_cross_oscillator(indicator_id, "DSP", 0.1)),
        IndicatorId::Ewmac => Some(zero_cross_oscillator(indicator_id, "EWMAC", 0.1)),
        IndicatorId::EwmacRobust => Some(zero_cross_oscillator(indicator_id, "EWMAC Robust", 0.1)),
        IndicatorId::Amat => Some(zero_cross_oscillator(indicator_id, "AMAT", 0.1)),
        IndicatorId::MoFisher => Some(zero_cross_oscillator(indicator_id, "MO Fisher", 0.1)),
        IndicatorId::MomZscore => Some(zero_cross_threshold(indicator_id, "Mom Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Pressure => Some(zero_cross_oscillator(indicator_id, "Pressure", 0.1)),
        IndicatorId::RsiZscore => Some(zero_cross_threshold(indicator_id, "RSI Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Hmom => Some(zero_cross_oscillator(indicator_id, "Hilbert Momentum", 0.01)),
        IndicatorId::Cusum => Some(zero_cross_oscillator(indicator_id, "CUSUM", 1.0)),
        IndicatorId::Roof => Some(zero_cross_oscillator(indicator_id, "Roofing Filter", 0.01)),
        IndicatorId::Mrf => Some(zero_cross_oscillator(indicator_id, "Mean Reversion Filter", 0.1)),
        IndicatorId::Zmad => Some(zero_cross_threshold(indicator_id, "ZMAD", 0.1, 2.0, -2.0)),
        IndicatorId::Wave => Some(zero_cross_oscillator(indicator_id, "Wave", 0.01)),
        IndicatorId::Sbp => Some(zero_cross_oscillator(indicator_id, "SBP", 0.01)),
        IndicatorId::Sbprhl => Some(zero_cross_oscillator(indicator_id, "SBPRHL", 0.01)),
        IndicatorId::Cyber => Some(zero_cross_oscillator(indicator_id, "Cyber Cycle", 0.01)),
        IndicatorId::Asi => Some(zero_cross_oscillator(indicator_id, "ASI", 1.0)),
        IndicatorId::Cho => Some(zero_cross_oscillator(indicator_id, "Chaikin Osc", 1000.0)),
        IndicatorId::Eom => Some(zero_cross_oscillator(indicator_id, "Ease of Movement", 0.01)),
        IndicatorId::Di => Some(zero_cross_oscillator(indicator_id, "Demand Index", 0.5)),
        IndicatorId::Pfe => Some(zero_cross_threshold(indicator_id, "PFE", 2.0, 50.0, -50.0)),
        IndicatorId::Smi => Some(smi_profile()),
        IndicatorId::EhlersRocket => Some(zero_cross_threshold(indicator_id, "Ehlers Rocket RSI", 0.02, 0.5, -0.5)),
        IndicatorId::Ravi => Some(zero_cross_oscillator(indicator_id, "RAVI", 0.1)),
        IndicatorId::AdxSlope => Some(zero_cross_oscillator(indicator_id, "ADX Slope", 0.01)),
        IndicatorId::KamaSlope => Some(zero_cross_oscillator(indicator_id, "KAMA Slope", 0.0001)),
        IndicatorId::EmaSlope => Some(zero_cross_oscillator(indicator_id, "EMA Slope", 0.0001)),
        IndicatorId::Didi => Some(didi_profile()),
        IndicatorId::TrEr => Some(threshold_only(indicator_id, "Trend vs ER", 0.7, 0.3)),
        IndicatorId::Atrz => Some(zero_cross_threshold(indicator_id, "ATR Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Rvz => Some(zero_cross_threshold(indicator_id, "RV Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Kscr => Some(zero_cross_oscillator(indicator_id, "Kalman Score", 0.1)),
        IndicatorId::Kslope => Some(zero_cross_oscillator(indicator_id, "Kalman Slope", 0.0001)),
        IndicatorId::Kslopez => Some(zero_cross_threshold(indicator_id, "Kalman Slope Z", 0.1, 2.0, -2.0)),
        IndicatorId::PriceZscore => Some(zero_cross_threshold(indicator_id, "Price Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Autocorr => Some(zero_cross_threshold(indicator_id, "Autocorrelation", 0.05, 0.5, -0.5)),
        IndicatorId::Pacf => Some(zero_cross_threshold(indicator_id, "PACF", 0.05, 0.3, -0.3)),
        IndicatorId::AvwapDist => Some(zero_cross_oscillator(indicator_id, "AVWAP Distance", 0.001)),
        IndicatorId::VwapDist => Some(zero_cross_oscillator(indicator_id, "VWAP Distance", 0.001)),
        IndicatorId::DistLevels => Some(zero_cross_oscillator(indicator_id, "Distance to Levels", 0.001)),
        IndicatorId::Sslope => Some(zero_cross_oscillator(indicator_id, "Spectral Slope", 0.01)),
        IndicatorId::Sslopez => Some(zero_cross_threshold(indicator_id, "Spectral Slope Z", 0.1, 2.0, -2.0)),
        IndicatorId::StCusum => Some(zero_cross_oscillator(indicator_id, "Structural CUSUM", 1.0)),
        IndicatorId::BpCusum => Some(zero_cross_oscillator(indicator_id, "BP CUSUM", 1.0)),
        IndicatorId::ResidStat => Some(zero_cross_threshold(indicator_id, "Residual Stat", 0.1, 2.0, -2.0)),
        IndicatorId::KpssZ => Some(zero_cross_threshold(indicator_id, "KPSS Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::VrZAgg => Some(zero_cross_threshold(indicator_id, "VR Z-Score Agg", 0.1, 2.0, -2.0)),
        IndicatorId::BookSlope => Some(zero_cross_oscillator(indicator_id, "Book Slope", 0.001)),
        IndicatorId::Sent => Some(zero_cross_threshold(indicator_id, "Sentiment", 2.0, 20.0, -20.0)),
        IndicatorId::Sentr => Some(zero_cross_oscillator(indicator_id, "Sentiment Rate", 0.1)),
        IndicatorId::MacdHistZ => Some(zero_cross_oscillator(indicator_id, "MACD Hist Z", 0.1)),
        IndicatorId::Qstick => Some(zero_cross_oscillator(indicator_id, "Qstick", 0.01)),
        IndicatorId::RocPct => Some(zero_cross_oscillator(indicator_id, "ROC Pct", 0.1)),
        IndicatorId::Cog => Some(zero_cross_oscillator(indicator_id, "COG", 0.01)),
        IndicatorId::Gapo => Some(volatility_swing(indicator_id, "GAPO")),
        IndicatorId::DiPlusMinus => Some(dual_line_crossover(indicator_id, "DI+/DI-")),
        IndicatorId::Dm => Some(zero_cross_oscillator(indicator_id, "DM", 0.01)),
        IndicatorId::Highest => Some(swing_levels(indicator_id, "Highest")),
        IndicatorId::Lowest => Some(swing_levels(indicator_id, "Lowest")),

        // Momentum — misc
        IndicatorId::RsiPctBands => Some(rsi_pct_bands_profile()),
        IndicatorId::BbPeriod => None, // informational metadata only
        IndicatorId::SwingAge => None,  // informational, not tradeable
        IndicatorId::SweepRev => Some(histogram_signal(indicator_id, "Sweep Reversal")),
        IndicatorId::Rwi => Some(rwi_profile()),
        IndicatorId::Vhf => Some(threshold_only(indicator_id, "VHF", 0.4, 0.2)),
        IndicatorId::VhfMa => Some(threshold_only(indicator_id, "VHF MA", 0.4, 0.2)),

        // ============================================================
        // Signal Processing
        // ============================================================
        IndicatorId::Butter => Some(overlay_crossover(indicator_id, "Butterworth")),
        IndicatorId::Cheby => Some(overlay_crossover(indicator_id, "Chebyshev")),
        IndicatorId::Decyc => Some(overlay_crossover(indicator_id, "Decycler")),
        IndicatorId::Ess => Some(overlay_crossover(indicator_id, "Super Smoother")),
        IndicatorId::Sg => Some(overlay_crossover(indicator_id, "Savitzky-Golay")),
        IndicatorId::Sbwf => Some(overlay_crossover(indicator_id, "Whittaker Filter")),
        IndicatorId::Hampel => Some(overlay_crossover(indicator_id, "Hampel")),
        IndicatorId::Rc => Some(overlay_crossover(indicator_id, "Renko")),
        IndicatorId::Scf => Some(volatility_swing(indicator_id, "SCF")),
        IndicatorId::Screst => Some(volatility_swing(indicator_id, "Spectral Crest")),
        IndicatorId::Sflux => Some(volatility_swing(indicator_id, "Spectral Flux")),
        IndicatorId::Shmpr => Some(volatility_swing(indicator_id, "SHMPR")),
        IndicatorId::Slmpr => Some(volatility_swing(indicator_id, "SLMPR")),
        IndicatorId::Sroll => Some(volatility_swing(indicator_id, "Spectral Rolloff")),
        IndicatorId::Sroll95 => Some(volatility_swing(indicator_id, "Sroll 95")),
        IndicatorId::Srollp => Some(threshold_only(indicator_id, "Srollp", 80.0, 20.0)),
        IndicatorId::Srollrp => Some(volatility_swing(indicator_id, "Srollrp")),
        IndicatorId::Stft => None, // frequency domain — no universal threshold
        IndicatorId::Fft => None,  // frequency domain — no universal threshold
        IndicatorId::Hdc => Some(volatility_swing(indicator_id, "Homodyne DC")),
        IndicatorId::Hilb => None, // multi-component spectral — visual only
        IndicatorId::Lz => Some(volatility_swing(indicator_id, "Lempel-Ziv")),
        IndicatorId::Sentent => Some(volatility_swing(indicator_id, "Sentent Entropy")),
        IndicatorId::Logicand => Some(threshold_only(indicator_id, "Logic AND", 0.9, 0.1)),
        IndicatorId::Logicor => Some(threshold_only(indicator_id, "Logic OR", 0.9, 0.1)),
        IndicatorId::Logicxor => Some(threshold_only(indicator_id, "Logic XOR", 0.9, 0.1)),
        IndicatorId::Logicsign => Some(zero_cross_oscillator(indicator_id, "Logic Sign", 0.5)),
        IndicatorId::Hyst => Some(swing_levels(indicator_id, "Hyst")),
        IndicatorId::Tenc => Some(zero_cross_threshold(indicator_id, "TENC", 0.05, 0.5, -0.5)),
        IndicatorId::Thresh => Some(threshold_only(indicator_id, "Thresh", 0.7, 0.3)),
        IndicatorId::Esine => Some(zero_cross_oscillator(indicator_id, "Even Sinewave", 0.01)),
        IndicatorId::Ser => Some(zero_cross_oscillator(indicator_id, "SER", 0.01)),
        IndicatorId::Sflat => Some(threshold_only(indicator_id, "Spectral Flatness", 0.7, 0.3)),

        // ============================================================
        // Channels / Bands
        // ============================================================
        IndicatorId::Bb => Some(bollinger_profile()),
        IndicatorId::Kc => Some(keltner_profile()),
        IndicatorId::Dc => Some(donchian_profile()),
        IndicatorId::Atrchan => Some(atr_channel_profile()),
        IndicatorId::Adaptivebb => Some(channel_indicator(indicator_id, "Adaptive BB")),
        IndicatorId::Adaptivechan => Some(channel_indicator(indicator_id, "Adaptive Chan")),
        IndicatorId::Starc => Some(channel_indicator(indicator_id, "STARC Bands")),
        IndicatorId::Envelope => Some(channel_indicator(indicator_id, "Envelope")),
        IndicatorId::Pricechan => Some(channel_indicator(indicator_id, "Price Channel")),
        IndicatorId::Regchan => Some(channel_indicator(indicator_id, "Regression Channel")),
        IndicatorId::Stddevchan => Some(channel_indicator(indicator_id, "StdDev Channel")),
        IndicatorId::Medchan => Some(channel_indicator(indicator_id, "Median Channel")),
        IndicatorId::Vwapchan => Some(channel_indicator(indicator_id, "VWAP Channel")),
        IndicatorId::Projbands => Some(channel_indicator(indicator_id, "Projection Bands")),
        IndicatorId::Qrchan => Some(channel_indicator(indicator_id, "QR Channel")),
        IndicatorId::Theilsenchan => Some(channel_indicator(indicator_id, "Theil-Sen Channel")),
        IndicatorId::Trimabands => Some(channel_indicator(indicator_id, "TRIMA Bands")),
        IndicatorId::Volprofchan => Some(channel_indicator(indicator_id, "Vol Profile Channel")),
        IndicatorId::Percentilech => Some(channel_indicator(indicator_id, "Percentile Channel")),
        IndicatorId::Dpobands => Some(channel_indicator(indicator_id, "DPO Bands")),
        IndicatorId::Atrc => Some(channel_indicator(indicator_id, "ATR Channels")),
        IndicatorId::Darvas => Some(darvas_profile()),
        IndicatorId::Vprb => Some(channel_indicator(indicator_id, "VPRB")),
        IndicatorId::Percentb => Some(threshold_only(indicator_id, "%B", 1.0, 0.0)),
        IndicatorId::Bbmetrics => Some(bbmetrics_profile()),
        IndicatorId::Kcmetrics => Some(volatility_swing(indicator_id, "Keltner Metrics")),
        IndicatorId::Dcmetrics => Some(volatility_swing(indicator_id, "DC Metrics")),
        IndicatorId::Dcpos => Some(threshold_only(indicator_id, "DC Position", 0.9, 0.1)),
        IndicatorId::Dcwidth => Some(volatility_swing(indicator_id, "DC Width")),
        IndicatorId::Keltpos => Some(threshold_only(indicator_id, "Keltner Position", 0.9, 0.1)),
        IndicatorId::Keltbw => Some(volatility_swing(indicator_id, "Keltner BW")),
        IndicatorId::Keltdist => Some(zero_cross_threshold(indicator_id, "Keltner Distance", 0.1, 2.0, -2.0)),
        IndicatorId::Medchanpos => Some(threshold_only(indicator_id, "Median Chan Pos", 0.9, 0.1)),
        IndicatorId::Pchwidth => Some(volatility_swing(indicator_id, "Price Chan Width")),
        IndicatorId::Pchosc => Some(zero_cross_threshold(indicator_id, "Price Chan Osc", 2.0, 50.0, -50.0)),
        IndicatorId::Regchanwidth => Some(volatility_swing(indicator_id, "Reg Chan Width")),
        IndicatorId::Stddevwidth => Some(volatility_swing(indicator_id, "StdDev Width")),
        IndicatorId::Vwapchanwidth => Some(volatility_swing(indicator_id, "VWAP Chan Width")),
        IndicatorId::Envbw => Some(volatility_swing(indicator_id, "Envelope BW")),
        IndicatorId::Ichimoku => Some(ichimoku_profile()),
        IndicatorId::Ichimokupos => Some(zero_cross_threshold(indicator_id, "Ichimoku Pos", 1.0, 5.0, -5.0)),
        IndicatorId::Ichimokuthick => Some(volatility_swing(indicator_id, "Ichimoku Thickness")),
        IndicatorId::Pivotchan => Some(pivot_chan_profile()),

        // ============================================================
        // Volatility
        // ============================================================
        IndicatorId::Atr => Some(atr_profile()),
        IndicatorId::Tr => Some(volatility_swing(indicator_id, "True Range")),
        IndicatorId::Natr => Some(volatility_swing(indicator_id, "NATR")),
        IndicatorId::Atrbw => Some(volatility_swing(indicator_id, "ATR BW")),
        IndicatorId::Atrp => Some(threshold_only(indicator_id, "ATR %", 3.0, 0.5)),
        IndicatorId::Atrpt => Some(overlay_crossover(indicator_id, "ATR % Trailing")),
        IndicatorId::Cv => Some(volatility_swing(indicator_id, "CV")),
        IndicatorId::Dvr => Some(volatility_swing(indicator_id, "Dynamic Vol Regime")),
        IndicatorId::Fuzzy => Some(volatility_swing(indicator_id, "Fuzzy")),
        IndicatorId::Har => Some(volatility_swing(indicator_id, "HAR")),
        IndicatorId::Hvc2c => Some(volatility_swing(indicator_id, "HVC2C")),
        IndicatorId::Kp => Some(volatility_swing(indicator_id, "Kase Peak")),
        IndicatorId::Nr => Some(threshold_only(indicator_id, "Narrow Range", 5.0, 1.0)),
        IndicatorId::Pgry => Some(volatility_swing(indicator_id, "PGRY")),
        IndicatorId::Rbvj => Some(volatility_swing(indicator_id, "Realized Bipower Jumps")),
        IndicatorId::Rcb => Some(volatility_swing(indicator_id, "Range Compression")),
        IndicatorId::Rp => Some(volatility_swing(indicator_id, "Realized Parkinson")),
        IndicatorId::Rq => Some(volatility_swing(indicator_id, "Realized Quarticity")),
        IndicatorId::Rv => Some(volatility_swing(indicator_id, "Realized Vol")),
        IndicatorId::Avr => Some(volatility_swing(indicator_id, "Average Vol Range")),
        IndicatorId::Bpv => Some(volatility_swing(indicator_id, "Bipower Variance")),
        IndicatorId::Ui => Some(threshold_only(indicator_id, "Ulcer Index", 10.0, 1.0)),
        IndicatorId::Vbd => Some(threshold_only(indicator_id, "Volatility Breakout", 1.0, 0.0)),
        IndicatorId::Vbexp => Some(volatility_swing(indicator_id, "Vol Expansion")),
        IndicatorId::VoDc => Some(volatility_swing(indicator_id, "Vol of DC")),
        IndicatorId::VoKc => Some(volatility_swing(indicator_id, "Vol of KC")),
        IndicatorId::VoVr => Some(volatility_swing(indicator_id, "Vol of Ratio")),
        IndicatorId::Vov => Some(volatility_swing(indicator_id, "Vol of Vol")),
        IndicatorId::Vovpt => Some(volatility_swing(indicator_id, "Vol of Vol % Trail")),
        IndicatorId::Wvf => Some(threshold_only(indicator_id, "Williams VixFix", 15.0, 3.0)),
        IndicatorId::Sqmom => Some(sqmom_profile()),
        IndicatorId::Chop => Some(threshold_only(indicator_id, "Choppiness Index", 61.8, 38.2)),
        IndicatorId::Rvol => Some(threshold_only(indicator_id, "RVOL", 2.0, 0.5)),
        IndicatorId::Garch => Some(volatility_swing(indicator_id, "GARCH")),
        IndicatorId::Egarch => Some(volatility_swing(indicator_id, "EGARCH")),
        IndicatorId::Var => Some(volatility_swing(indicator_id, "VAR")),
        IndicatorId::Hurst => Some(threshold_only(indicator_id, "Hurst", 0.6, 0.4)),
        IndicatorId::FractalDim => Some(threshold_only(indicator_id, "Fractal Dim", 1.6, 1.4)),
        IndicatorId::Dfa => Some(volatility_swing(indicator_id, "DFA")),
        IndicatorId::Rvi => Some(threshold_only(indicator_id, "RVI", 60.0, 40.0)),

        // ============================================================
        // Levels / Support-Resistance (overlay)
        // ============================================================
        IndicatorId::Avwap => Some(avwap_profile()),
        IndicatorId::Bos => Some(swing_levels(indicator_id, "BOS")),
        IndicatorId::Camarilla => Some(swing_levels(indicator_id, "Camarilla")),
        IndicatorId::Demark => Some(swing_levels(indicator_id, "DeMark")),
        IndicatorId::Floorpivot => Some(swing_levels(indicator_id, "Floor Pivot")),
        IndicatorId::Fvg => Some(fvg_profile()),
        IndicatorId::Fvgalt => Some(swing_levels(indicator_id, "FVG Alt")),
        IndicatorId::Fvgdur => Some(threshold_only(indicator_id, "FVG Duration", 10.0, 1.0)),
        IndicatorId::Fvgrev => Some(histogram_signal(indicator_id, "FVG Reversal")),
        IndicatorId::Liqgap => Some(swing_levels(indicator_id, "Liquidity Gap")),
        IndicatorId::Pivavwap => Some(piv_avwap_profile()),
        IndicatorId::Pivot => None, // static price levels — visual only
        IndicatorId::Rmid => Some(overlay_crossover(indicator_id, "Range Mid")),
        IndicatorId::Rquart => Some(range_quart_profile()),
        IndicatorId::Swingstr => Some(threshold_only(indicator_id, "Swing Strength", 70.0, 30.0)),
        IndicatorId::Woodie => Some(swing_levels(indicator_id, "Woodie")),
        IndicatorId::Cpr => Some(cpr_profile()),
        IndicatorId::VwapLevels => None, // static VWAP price levels — visual only

        // ============================================================
        // Candle Patterns
        // ============================================================
        IndicatorId::Candleanatomy => None, // body/wick sizes — visual only
        IndicatorId::Heikinashi => None, // price-level overlay — visual only
        IndicatorId::Wickspike => Some(histogram_signal(indicator_id, "Wick Spike")),
        IndicatorId::HaTrend => Some(histogram_signal(indicator_id, "HA Trend")),

        // ============================================================
        // Volume indicators
        // ============================================================
        IndicatorId::Obv => Some(obv_profile()),
        IndicatorId::Cmf => Some(cmf_profile()),
        IndicatorId::Ad => Some(ad_profile()),
        IndicatorId::Fi => Some(zero_cross_oscillator(indicator_id, "Force Index", 1000.0)),
        IndicatorId::Kvo => Some(kvo_profile()),
        IndicatorId::Vroc => Some(zero_cross_threshold(indicator_id, "Volume ROC", 1.0, 50.0, -50.0)),
        IndicatorId::NviPvi => Some(nvi_pvi_profile()),
        IndicatorId::Tmf => Some(zero_cross_threshold(indicator_id, "Twiggs Money Flow", 0.01, 0.2, -0.2)),
        IndicatorId::Pvo => Some(zero_cross_oscillator(indicator_id, "PVO", 1.0)),
        IndicatorId::Pzo => Some(zero_cross_threshold(indicator_id, "PZO", 2.0, 15.0, -15.0)),
        IndicatorId::Vdelta => Some(vdelta_profile()),
        IndicatorId::Vfi => Some(zero_cross_threshold(indicator_id, "VFI", 0.05, 0.2, -0.2)),
        IndicatorId::Vo => Some(zero_cross_oscillator(indicator_id, "Volume Osc", 1.0)),
        IndicatorId::Vpin => Some(threshold_only(indicator_id, "VPIN", 0.6, 0.2)),
        IndicatorId::Vprofile => None, // histogram of volume at price — visual only
        IndicatorId::Vpt => Some(divergence_swing(indicator_id, "VPT")),
        IndicatorId::Vz => Some(zero_cross_threshold(indicator_id, "Volume Z-Score", 0.1, 2.0, -2.0)),
        IndicatorId::Vzo => Some(zero_cross_threshold(indicator_id, "VZO", 2.0, 5.0, -5.0)),
        IndicatorId::Poc => Some(swing_levels(indicator_id, "POC")),
        IndicatorId::Wad => Some(divergence_swing(indicator_id, "Williams A/D")),
        IndicatorId::Pvt => Some(divergence_swing(indicator_id, "PVT")),
        IndicatorId::Ii => Some(zero_cross_oscillator(indicator_id, "Intraday Intensity", 1000.0)),
        IndicatorId::Iip => Some(zero_cross_threshold(indicator_id, "II Percent", 2.0, 20.0, -20.0)),
        IndicatorId::Iir => Some(threshold_only(indicator_id, "II Ratio", 1.5, 0.5)),
        IndicatorId::BookImb => Some(book_imb_profile()),
        IndicatorId::ClQueueImb => Some(cl_queue_imb_profile()),
        IndicatorId::OrderFlowImb => Some(zero_cross_oscillator(indicator_id, "Order Flow Imb", 0.1)),
        IndicatorId::MarketMicro => None, // composite microstructure — informational
        IndicatorId::TickVolume => Some(threshold_only(indicator_id, "Tick Volume", 2.0, 0.5)),

        // ============================================================
        // Trend indicators
        // ============================================================
        IndicatorId::GannHilo => Some(overlay_crossover(indicator_id, "Gann HiLo")),
        IndicatorId::Gmma => Some(overlay_crossover(indicator_id, "GMMA")),
        IndicatorId::Sdl => Some(overlay_crossover(indicator_id, "SDL")),
        IndicatorId::Ssl => Some(ssl_profile()),
        IndicatorId::Alligator => Some(alligator_profile()),
        IndicatorId::Eit => Some(eit_profile()),
        IndicatorId::ElderImpulse => Some(histogram_signal(indicator_id, "Elder Impulse")),
        IndicatorId::Kregime => Some(kregime_profile()),

        // ============================================================
        // Accumulation
        // ============================================================
        // (Most handled in Volume above; remaining here)

        // ============================================================
        // Adaptive / Kalman
        // ============================================================
        IndicatorId::Mama => Some(overlay_crossover(indicator_id, "MAMA")),
        IndicatorId::Kalman => Some(overlay_crossover(indicator_id, "Kalman")),
        IndicatorId::Ekf => Some(overlay_crossover(indicator_id, "EKF")),
        IndicatorId::Ukf => Some(overlay_crossover(indicator_id, "UKF")),
        IndicatorId::Abgfilter => Some(overlay_crossover(indicator_id, "ABG Filter")),
        IndicatorId::Kcomp => Some(overlay_crossover(indicator_id, "KCOMP")),
        IndicatorId::Particle => Some(overlay_crossover(indicator_id, "Particle Filter")),
        IndicatorId::Rts => Some(overlay_crossover(indicator_id, "RTS Smoother")),

        // ============================================================
        // Trend Stop
        // ============================================================
        IndicatorId::Atrts => Some(overlay_crossover(indicator_id, "ATR Trail Stop")),
        IndicatorId::Chand => Some(chand_profile()),
        IndicatorId::Cks => Some(overlay_crossover(indicator_id, "CKS Stop")),
        IndicatorId::Donbo => Some(donbo_profile()),
        IndicatorId::Dons => Some(overlay_crossover(indicator_id, "Donchian Stop")),
        IndicatorId::Kelts => Some(overlay_crossover(indicator_id, "Keltner Stop")),
        IndicatorId::Psars => Some(overlay_crossover(indicator_id, "PSAR Stop")),
        IndicatorId::Supts => Some(overlay_crossover(indicator_id, "Support Stop")),
        IndicatorId::TsSwings => Some(ts_swings_profile()),
        IndicatorId::Volts => Some(overlay_crossover(indicator_id, "Volatility Stop")),

        // ============================================================
        // Chaos / Fractal
        // ============================================================
        IndicatorId::Ac => Some(ac_profile()),
        IndicatorId::ChaosOsc => Some(zero_cross_oscillator(indicator_id, "Chaos Oscillator", 0.1)),
        IndicatorId::Fractals => Some(fractals_profile()),
        IndicatorId::WilliamsMfi => Some(williams_mfi_profile()),


        // ============================================================
        // Regression
        // ============================================================
        IndicatorId::Arima => Some(overlay_crossover(indicator_id, "ARIMA")),
        IndicatorId::PolyReg => Some(overlay_crossover(indicator_id, "PolyReg")),

        // ============================================================
        // Ratio / Efficiency
        // ============================================================
        IndicatorId::Er => Some(threshold_only(indicator_id, "Efficiency Ratio", 0.7, 0.3)),
        IndicatorId::ErRing => Some(threshold_only(indicator_id, "ER Ring", 0.7, 0.3)),
        IndicatorId::RangeAtr => Some(threshold_only(indicator_id, "Range/ATR", 1.5, 0.7)),

        // ============================================================
        // Entropy / Information Theory
        // ============================================================
        IndicatorId::Apen => Some(volatility_swing(indicator_id, "Approx Entropy")),
        IndicatorId::Conden => Some(volatility_swing(indicator_id, "Conditional Entropy")),
        IndicatorId::Fisher => Some(zero_cross_oscillator(indicator_id, "Fisher", 0.01)),
        IndicatorId::Infog => Some(volatility_swing(indicator_id, "Information Gain")),
        IndicatorId::Jsd => Some(threshold_only(indicator_id, "JS Divergence", 0.5, 0.1)),
        IndicatorId::Kld => Some(volatility_swing(indicator_id, "KL Divergence")),
        IndicatorId::Mi => Some(volatility_swing(indicator_id, "Mutual Information")),
        IndicatorId::Pe => Some(volatility_swing(indicator_id, "Permutation Entropy")),
        IndicatorId::Sampen => Some(volatility_swing(indicator_id, "Sample Entropy")),
        IndicatorId::Shannon => Some(volatility_swing(indicator_id, "Shannon Entropy")),
        IndicatorId::Te => Some(volatility_swing(indicator_id, "Transfer Entropy")),
        IndicatorId::Xmil => Some(volatility_swing(indicator_id, "Cross MI")),

        // ============================================================
        // Statistics / Econometric
        // ============================================================
        IndicatorId::Adf => Some(threshold_only(indicator_id, "ADF", -1.9, -3.5)),
        IndicatorId::AdfKpss => Some(threshold_only(indicator_id, "ADF-KPSS", 0.7, 0.3)),
        IndicatorId::ArchLm => Some(threshold_only(indicator_id, "ARCH LM", 3.8, 0.0)),
        IndicatorId::ArchLmPval => Some(threshold_only(indicator_id, "ARCH LM P-Value", 0.1, 0.05)),
        IndicatorId::Coint => Some(threshold_only(indicator_id, "Cointegration", 0.0, -3.0)),
        IndicatorId::EgAdf => Some(threshold_only(indicator_id, "EG ADF", 0.0, -3.0)),
        IndicatorId::EgCoint => Some(threshold_only(indicator_id, "EG Cointegration", 0.0, -3.0)),
        IndicatorId::EgTrend => Some(threshold_only(indicator_id, "EG Trend", 0.7, 0.3)),
        IndicatorId::HalfLifeMr => Some(threshold_only(indicator_id, "Half-Life MR", 100.0, 5.0)),
        IndicatorId::Kpss => Some(threshold_only(indicator_id, "KPSS", 0.739, 0.119)),
        IndicatorId::KpssTrend => Some(threshold_only(indicator_id, "KPSS Trend", 0.739, 0.119)),
        IndicatorId::LjungBox => Some(threshold_only(indicator_id, "Ljung-Box", 20.0, 0.0)),
        IndicatorId::Pp => Some(threshold_only(indicator_id, "Phillips-Perron", -1.9, -3.5)),
        IndicatorId::PvCoherence => Some(threshold_only(indicator_id, "PV Coherence", 0.7, 0.3)),
        IndicatorId::RSquared => Some(threshold_only(indicator_id, "R-Squared", 0.7, 0.3)),
        IndicatorId::Vr => Some(threshold_only(indicator_id, "Variance Ratio", 1.2, 0.8)),
        IndicatorId::VrAgg => Some(threshold_only(indicator_id, "VR Aggregate", 1.2, 0.8)),
        IndicatorId::Za => Some(threshold_only(indicator_id, "Zivot-Andrews", -1.9, -4.0)),

        // ============================================================
        // Clusters / Order Book
        // ============================================================
        IndicatorId::DomWoq => None, // microstructure depth data — no clean threshold

        // ============================================================
        // Position / Calendar
        // ============================================================
        IndicatorId::HolidayProx => Some(threshold_only(indicator_id, "Holiday Proximity", 0.8, 0.2)),
        IndicatorId::HourDay => Some(threshold_only(indicator_id, "Hour of Day", 16.0, 9.0)),
        IndicatorId::MonthQtr => Some(threshold_only(indicator_id, "Month of Quarter", 3.0, 1.0)),
        IndicatorId::MonthTurn => Some(threshold_only(indicator_id, "Month Turn", 0.9, 0.1)),
        IndicatorId::QtrTurn => Some(threshold_only(indicator_id, "Quarter Turn", 0.9, 0.1)),
        IndicatorId::Session => None, // categorical session ID — not meaningful
        IndicatorId::SomEom => Some(threshold_only(indicator_id, "SOM/EOM", 0.9, 0.1)),
        IndicatorId::SoqEoq => Some(threshold_only(indicator_id, "SOQ/EOQ", 0.9, 0.1)),
        IndicatorId::SowEow => Some(threshold_only(indicator_id, "SOW/EOW", 0.9, 0.1)),
        IndicatorId::WeekMonth => Some(threshold_only(indicator_id, "Week of Month", 4.0, 2.0)),
        IndicatorId::Weekday => Some(threshold_only(indicator_id, "Weekday", 5.0, 2.0)),

        // Catch-all for truly unknown/future variants
        _ => None,
    }
}

// ============================================================================
// Shared helper — divergence + swing
// ============================================================================

fn divergence_swing(id: IndicatorId, name: &str) -> SignalProfile {
    SignalProfile::new(id, format!("{} Default", name))
        .with_description(format!("{}: divergence and swing signals", name))
        .with_detectors([
            DetectorConfig::divergence("divergence", format!("{} Divergence", name), ValueSource::Main, 14),
            DetectorConfig::swing("swing", format!("{} Swings", name), ValueSource::Main, 5),
        ])
}

// ============================================================================
// Momentum Indicators
// ============================================================================

/// RSI default profile - overbought/oversold with divergence
pub fn rsi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Rsi, "RSI Default")
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
    SignalProfile::new(IndicatorId::Stoch, "Stochastic Default")
        .with_description("Stochastic signals: K/D crossovers, overbought/oversold zones")
        .with_detectors([
            DetectorConfig::crossover("kd_cross", "%K/%D Crossover", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::First, 80.0, 20.0),
            DetectorConfig::divergence("divergence", "Stochastic Divergence", ValueSource::First, 14),
        ])
}

/// Stochastic RSI profile
pub fn stoch_rsi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::StochRsi, "StochRSI Default")
        .with_description("Stochastic RSI: K/D crossovers and overbought/oversold")
        .with_detectors([
            DetectorConfig::crossover("kd_cross", "K/D Crossover", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::First, 0.8, 0.2),
        ])
}

/// CCI default profile - zero cross and extreme levels
pub fn cci_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Cci, "CCI Default")
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
    SignalProfile::new(IndicatorId::Mfi, "MFI Default")
        .with_description("Money Flow Index: overbought/oversold levels")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 80.0, 20.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, 90.0, 10.0),
            DetectorConfig::divergence("divergence", "MFI Divergence", ValueSource::Main, 14),
        ])
}

/// Williams %R default profile
pub fn williams_r_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::WilliamsR, "Williams %R Default")
        .with_description("Williams %R: overbought/oversold levels (-20/-80)")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, -20.0, -80.0),
            DetectorConfig::threshold("extreme", "Extreme Levels", ValueSource::Main, -10.0, -90.0),
        ])
}

/// CMO (Chande Momentum Oscillator) default profile
pub fn cmo_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Cmo, "CMO Default")
        .with_description("Chande Momentum Oscillator: zero cross and overbought/oversold")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 2.0),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 50.0, -50.0),
        ])
}

/// ROC (Rate of Change) default profile
pub fn roc_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Roc, "ROC Default")
        .with_description("Rate of Change: zero line crossings")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
            DetectorConfig::divergence("divergence", "ROC Divergence", ValueSource::Main, 10),
        ])
}

/// SMI (Stochastic Momentum Index) profile
pub fn smi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Smi, "SMI Default")
        .with_description("SMI: line crossover and overbought/oversold")
        .with_detectors([
            DetectorConfig::crossover("cross", "SMI Crossover", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::First, 40.0, -40.0),
        ])
}

/// RSI Pct Bands profile
pub fn rsi_pct_bands_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::RsiPctBands, "RSI Pct Bands Default")
        .with_description("RSI Percentile Bands: RSI within channel bands")
        .with_detectors([
            DetectorConfig::threshold("ob_os", "RSI Levels", ValueSource::ChannelMiddle, 70.0, 30.0),
            DetectorConfig::channel("channel", "Band Position", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// RWI profile
pub fn rwi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Rwi, "RWI Default")
        .with_description("Random Walk Index: crossover and trend threshold")
        .with_detectors([
            DetectorConfig::crossover("cross", "RWI High/Low Cross", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("levels", "Trend Level", ValueSource::First, 1.0, 0.5),
        ])
}

/// DIDI Index profile
pub fn didi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Didi, "DIDI Default")
        .with_description("DIDI Index: crossover and zero cross on short ratio")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::First, 0.001),
            DetectorConfig::crossover("cross", "DIDI Crossover", ValueSource::First, ValueSource::Second),
        ])
}

// ============================================================================
// Trend Indicators
// ============================================================================

/// MACD default profile - comprehensive signals
pub fn macd_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Macd, "MACD Default")
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
    SignalProfile::new(IndicatorId::Adx, "ADX Default")
        .with_description("ADX signals: trend strength levels, DI crossovers")
        .with_detectors([
            DetectorConfig::threshold("trend_strength", "Trend Strength", ValueSource::Main, 25.0, 20.0),
            DetectorConfig::threshold("strong_trend", "Strong Trend", ValueSource::Main, 40.0, 15.0),
        ])
}

/// Aroon default profile
pub fn aroon_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Aroon, "Aroon Default")
        .with_description("Aroon signals: up/down crossovers, extreme levels")
        .with_detectors([
            DetectorConfig::crossover("aroon_cross", "Aroon Up/Down Cross", ValueSource::First, ValueSource::Second),
            DetectorConfig::threshold("strong_up", "Strong Uptrend", ValueSource::First, 70.0, 30.0),
            DetectorConfig::threshold("strong_down", "Strong Downtrend", ValueSource::Second, 70.0, 30.0),
        ])
}

/// SuperTrend default profile
pub fn supertrend_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Supertrend, "SuperTrend Default")
        .with_description("SuperTrend signals: trend direction changes")
        .with_detectors([
            DetectorConfig::zero_cross("direction", "Trend Direction Change", ValueSource::Main, 0.5),
        ])
}

/// Parabolic SAR default profile
pub fn psar_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Psar, "Parabolic SAR Default")
        .with_description("Parabolic SAR: trend reversal signals")
        .with_detectors([
            DetectorConfig::zero_cross("reversal", "SAR Reversal", ValueSource::Main, 0.1),
        ])
}

/// SSL Channel profile
pub fn ssl_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ssl, "SSL Default")
        .with_description("SSL Channel: up/down line crossovers")
        .with_detectors([
            DetectorConfig::crossover("cross", "SSL Crossover", ValueSource::First, ValueSource::Second),
        ])
}

/// Alligator profile
pub fn alligator_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Alligator, "Alligator Default")
        .with_description("Alligator: Lips cross Jaw signal")
        .with_detectors([
            DetectorConfig::crossover("lips_jaw", "Lips/Jaw Cross", ValueSource::First, ValueSource::Third),
        ])
}

/// Elder Impulse + Trend profile
pub fn eit_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Eit, "EIT Default")
        .with_description("Elder Impulse Trend: composite histogram trend score")
        .with_detectors([
            DetectorConfig::histogram("signal", "EIT Signal", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Zero Cross", ValueSource::Main, 0.5),
        ])
}

/// Kalman Regime profile
pub fn kregime_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Kregime, "Kalman Regime Default")
        .with_description("Kalman Regime: regime change signals")
        .with_detectors([
            DetectorConfig::histogram("signal", "Regime Signal", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Regime Change", ValueSource::Main, 0.1),
        ])
}

// ============================================================================
// Channel Indicators
// ============================================================================

/// Bollinger Bands default profile
pub fn bollinger_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Bb, "Bollinger Bands Default")
        .with_description("Bollinger Bands: band touches, squeeze, expansion")
        .with_detectors([
            DetectorConfig::channel("band_touch", "Band Touch", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// Keltner Channel default profile
pub fn keltner_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Kc, "Keltner Channel Default")
        .with_description("Keltner Channel: channel breakouts and returns")
        .with_detectors([
            DetectorConfig::channel("channel", "Channel Position", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// Donchian Channel default profile
pub fn donchian_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Dc, "Donchian Channel Default")
        .with_description("Donchian Channel: breakout signals")
        .with_detectors([
            DetectorConfig::channel("breakout", "Channel Breakout", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// ATR Channel default profile
pub fn atr_channel_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Atrchan, "ATR Channel Default")
        .with_description("ATR-based channel: volatility envelope signals")
        .with_detectors([
            DetectorConfig::channel("channel", "ATR Channel", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

/// Darvas Box profile
pub fn darvas_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Darvas, "Darvas Box Default")
        .with_description("Darvas Box: box breakout signals")
        .with_detectors([
            DetectorConfig::channel("box", "Box Breakout", ValueSource::First, ValueSource::Second),
        ])
}

/// BB Metrics profile
pub fn bbmetrics_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Bbmetrics, "BB Metrics Default")
        .with_description("BB Metrics: %B levels and bandwidth changes")
        .with_detectors([
            DetectorConfig::threshold("pct_b", "%B Levels", ValueSource::First, 1.0, 0.0),
            DetectorConfig::swing("bw_swing", "Bandwidth Swings", ValueSource::Second, 5),
        ])
}

/// Pivot Channel profile
pub fn pivot_chan_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Pivotchan, "Pivot Channel Default")
        .with_description("Pivot Channel: R1/S1 channel overlay")
        .with_detectors([
            DetectorConfig::channel("channel", "Pivot Channel", ValueSource::ChannelUpper, ValueSource::ChannelLower),
        ])
}

// ============================================================================
// Ichimoku
// ============================================================================

/// Ichimoku Cloud default profile - comprehensive signals
pub fn ichimoku_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ichimoku, "Ichimoku Default")
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
    SignalProfile::new(IndicatorId::Obv, "OBV Default")
        .with_description("On Balance Volume: trend confirmation and divergences")
        .with_detectors([
            DetectorConfig::divergence("divergence", "OBV Divergence", ValueSource::Main, 14),
            DetectorConfig::swing("swing", "OBV Swings", ValueSource::Main, 5),
        ])
}

/// VWAP default profile
pub fn vwap_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Vwap, "VWAP Default")
        .with_description("VWAP: price crossover signals")
        .with_detectors([
            DetectorConfig::swing("swing", "VWAP Swings", ValueSource::Main, 5),
        ])
}

/// CMF default profile
pub fn cmf_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Cmf, "CMF Default")
        .with_description("Chaikin Money Flow: zero line cross and accumulation/distribution")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.01),
            DetectorConfig::threshold("strong", "Strong Flow", ValueSource::Main, 0.25, -0.25),
        ])
}

/// Accumulation/Distribution default profile
pub fn ad_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ad, "A/D Default")
        .with_description("Accumulation/Distribution: divergence signals")
        .with_detectors([
            DetectorConfig::divergence("divergence", "A/D Divergence", ValueSource::Main, 14),
        ])
}

/// KVO (Klinger Volume Oscillator) profile
pub fn kvo_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Kvo, "KVO Default")
        .with_description("Klinger Volume Oscillator: line crossover and zero cross")
        .with_detectors([
            DetectorConfig::crossover("cross", "KVO Crossover", ValueSource::First, ValueSource::Second),
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::First, 1000.0),
        ])
}

/// NVI/PVI profile
pub fn nvi_pvi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::NviPvi, "NVI/PVI Default")
        .with_description("NVI/PVI: NVI cross PVI signal")
        .with_detectors([
            DetectorConfig::crossover("cross", "NVI/PVI Crossover", ValueSource::First, ValueSource::Second),
        ])
}

/// Volume Delta profile
pub fn vdelta_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Vdelta, "Volume Delta Default")
        .with_description("Volume Delta: histogram sign changes and zero cross")
        .with_detectors([
            DetectorConfig::histogram("signal", "Volume Delta Signal", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Zero Cross", ValueSource::Main, 100.0),
        ])
}

/// Book Imbalance profile
pub fn book_imb_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::BookImb, "Book Imbalance Default")
        .with_description("Book Imbalance: histogram and threshold signals")
        .with_detectors([
            DetectorConfig::histogram("signal", "Imbalance Signal", ValueSource::Main),
            DetectorConfig::threshold("levels", "Imbalance Levels", ValueSource::Main, 0.3, -0.3),
        ])
}

/// CL Queue Imbalance profile
pub fn cl_queue_imb_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::ClQueueImb, "CL Queue Imbalance Default")
        .with_description("CL Queue Imbalance: histogram and zero cross")
        .with_detectors([
            DetectorConfig::histogram("signal", "CL Queue Signal", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Zero Cross", ValueSource::Main, 0.1),
        ])
}

// ============================================================================
// Volatility Indicators
// ============================================================================

/// ATR default profile
pub fn atr_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Atr, "ATR Default")
        .with_description("Average True Range: volatility expansion/contraction")
        .with_detectors([
            DetectorConfig::swing("volatility", "Volatility Changes", ValueSource::Main, 5),
        ])
}

/// Squeeze Momentum profile
pub fn sqmom_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Sqmom, "Squeeze Momentum Default")
        .with_description("Squeeze Momentum: histogram and zero cross")
        .with_detectors([
            DetectorConfig::histogram("signal", "Squeeze Signal", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Zero Cross", ValueSource::Main, 0.01),
        ])
}

// ============================================================================
// Levels / Support-Resistance
// ============================================================================

/// AVWAP profile
pub fn avwap_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Avwap, "AVWAP Default")
        .with_description("Anchored VWAP: price crossover and swing levels")
        .with_detectors([
            DetectorConfig::price_crossover("crossover", "Price Cross AVWAP"),
            DetectorConfig::swing("swing", "AVWAP Swings", ValueSource::Main, 5),
        ])
}

/// Pivot AVWAP profile
pub fn piv_avwap_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Pivavwap, "Pivot AVWAP Default")
        .with_description("Pivot AVWAP: price crossover and swing")
        .with_detectors([
            DetectorConfig::price_crossover("crossover", "Price Cross Pivot AVWAP"),
            DetectorConfig::swing("swing", "Pivot AVWAP Swings", ValueSource::Main, 5),
        ])
}

/// FVG (Fair Value Gap) profile
pub fn fvg_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Fvg, "FVG Default")
        .with_description("Fair Value Gap: gap zone channel")
        .with_detectors([
            DetectorConfig::channel("gap_zone", "FVG Zone", ValueSource::First, ValueSource::Second),
        ])
}

/// CPR (Central Pivot Range) profile
pub fn cpr_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Cpr, "CPR Default")
        .with_description("Central Pivot Range: BC/TC channel")
        .with_detectors([
            DetectorConfig::channel("channel", "CPR Channel", ValueSource::Second, ValueSource::Third),
        ])
}

/// Range Quartiles profile
pub fn range_quart_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Rquart, "Range Quartiles Default")
        .with_description("Range Quartiles: Q1/Q3 channel")
        .with_detectors([
            DetectorConfig::channel("channel", "Q1/Q3 Channel", ValueSource::First, ValueSource::Third),
        ])
}

// ============================================================================
// Moving Averages
// ============================================================================

/// Generic moving average profile
pub fn ma_profile(indicator_id: IndicatorId) -> SignalProfile {
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
    SignalProfile::new(IndicatorId::Tsi, "TSI Default")
        .with_description("True Strength Index: zero cross and signal line")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 2.0),
            DetectorConfig::threshold("ob_os", "Overbought/Oversold", ValueSource::Main, 25.0, -25.0),
        ])
}

/// PPO default profile
pub fn ppo_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ppo, "PPO Default")
        .with_description("Percentage Price Oscillator: zero cross")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
        ])
}

/// Awesome Oscillator default profile
pub fn ao_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ao, "AO Default")
        .with_description("Awesome Oscillator: zero cross and saucer signals")
        .with_detectors([
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.1),
            DetectorConfig::histogram("histogram", "AO Histogram", ValueSource::Main),
        ])
}

/// Accelerator Oscillator profile
pub fn ac_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Ac, "AC Default")
        .with_description("Accelerator Oscillator: histogram and zero cross")
        .with_detectors([
            DetectorConfig::histogram("signal", "AC Histogram", ValueSource::Main),
            DetectorConfig::zero_cross("zero", "Zero Line Cross", ValueSource::Main, 0.01),
        ])
}

/// Elder Ray profile
pub fn elder_ray_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::ElderRay, "Elder Ray Default")
        .with_description("Elder Ray: Bull/Bear power histogram and zero cross")
        .with_detectors([
            DetectorConfig::histogram("signal", "Elder Ray Signal", ValueSource::First),
            DetectorConfig::zero_cross("zero", "Bull Power Zero Cross", ValueSource::First, 0.1),
        ])
}

// ============================================================================
// Chaos / Fractal
// ============================================================================

/// Fractals profile
pub fn fractals_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Fractals, "Fractals Default")
        .with_description("Fractals: fractal high/low swing pivots")
        .with_detectors([
            DetectorConfig::swing("swing", "Fractal Pivots", ValueSource::First, 5),
        ])
}

/// Williams MFI profile
pub fn williams_mfi_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::WilliamsMfi, "Williams MFI Default")
        .with_description("Williams Market Facilitation Index: histogram signal")
        .with_detectors([
            DetectorConfig::histogram("signal", "Williams MFI Signal", ValueSource::Main),
            DetectorConfig::swing("swing", "MFI Swings", ValueSource::Main, 5),
        ])
}

// ============================================================================
// Trend Stop
// ============================================================================

/// Chandelier Exit profile
pub fn chand_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Chand, "Chandelier Exit Default")
        .with_description("Chandelier Exit: long stop price crossover")
        .with_detectors([
            DetectorConfig::crossover("cross", "Long Exit Cross", ValueSource::Price, ValueSource::First),
        ])
}

/// Donchian Breakout profile
pub fn donbo_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::Donbo, "Donchian Breakout Default")
        .with_description("Donchian Breakout: upper/lower channel")
        .with_detectors([
            DetectorConfig::channel("channel", "Donchian Channel", ValueSource::First, ValueSource::Second),
        ])
}

/// TS Swings profile
pub fn ts_swings_profile() -> SignalProfile {
    SignalProfile::new(IndicatorId::TsSwings, "TS Swings Default")
        .with_description("TS Swings: high/low stop channel")
        .with_detectors([
            DetectorConfig::channel("channel", "TS Swings Channel", ValueSource::First, ValueSource::Second),
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
        assert!(default_profile(IndicatorId::Rsi).is_some());
        assert!(default_profile(IndicatorId::Macd).is_some());
        assert!(default_profile(IndicatorId::Bb).is_some());
        assert!(default_profile(IndicatorId::Adx).is_some());
        assert!(default_profile(IndicatorId::Stoch).is_some());
        assert!(default_profile(IndicatorId::Ichimoku).is_some());
    }

    #[test]
    fn test_unknown_indicator_returns_none() {
        assert!(default_profile(IndicatorId::Session).is_none());
        assert!(default_profile(IndicatorId::Volume).is_none());
        assert!(default_profile(IndicatorId::DomWoq).is_none());
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
        for id in [
            IndicatorId::Sma,
            IndicatorId::Ema,
            IndicatorId::Wma,
            IndicatorId::Dema,
            IndicatorId::Tema,
            IndicatorId::Alma,
            IndicatorId::Jma,
            IndicatorId::T3,
        ] {
            assert!(default_profile(id).is_some(), "Missing profile for {:?}", id);
        }
    }

    #[test]
    fn test_new_oscillator_profiles() {
        assert!(default_profile(IndicatorId::StochRsi).is_some());
        assert!(default_profile(IndicatorId::ConnorsRsi).is_some());
        assert!(default_profile(IndicatorId::Qqe).is_some());
        assert!(default_profile(IndicatorId::Pmo).is_some());
        assert!(default_profile(IndicatorId::Kst).is_some());
        assert!(default_profile(IndicatorId::Stc).is_some());
    }

    #[test]
    fn test_channel_profiles() {
        assert!(default_profile(IndicatorId::Adaptivebb).is_some());
        assert!(default_profile(IndicatorId::Starc).is_some());
        assert!(default_profile(IndicatorId::Envelope).is_some());
        assert!(default_profile(IndicatorId::Regchan).is_some());
    }

    #[test]
    fn test_candle_patterns() {
        assert!(default_profile(IndicatorId::Wickspike).is_some());
        assert!(default_profile(IndicatorId::HaTrend).is_some());
    }

    #[test]
    fn test_volatility_profiles() {
        assert!(default_profile(IndicatorId::Chop).is_some());
        assert!(default_profile(IndicatorId::Hurst).is_some());
        assert!(default_profile(IndicatorId::Sqmom).is_some());
        assert!(default_profile(IndicatorId::Atrp).is_some());
    }

    #[test]
    fn test_trend_stop_profiles() {
        assert!(default_profile(IndicatorId::Atrts).is_some());
        assert!(default_profile(IndicatorId::Chand).is_some());
        assert!(default_profile(IndicatorId::Donbo).is_some());
        assert!(default_profile(IndicatorId::TsSwings).is_some());
    }

    #[test]
    fn test_stat_profiles() {
        assert!(default_profile(IndicatorId::Adf).is_some());
        assert!(default_profile(IndicatorId::Kpss).is_some());
        assert!(default_profile(IndicatorId::RSquared).is_some());
        assert!(default_profile(IndicatorId::Vr).is_some());
    }
}
