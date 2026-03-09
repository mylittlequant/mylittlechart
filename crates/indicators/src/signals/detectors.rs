//! Signal Detectors - утилиты для обнаружения сигналов
//!
//! Детекторы - это stateful объекты, которые отслеживают состояние
//! и генерируют сигналы при выполнении условий.

use crate::signals::{
    SignalKind, CrossoverType, DivergenceType,
    ChannelPosition, VolatilityRegime, VolumeCharacter,
};
use arrayvec::ArrayVec;

// ============================================================================
// CROSSOVER DETECTOR - Детектор пересечений
// ============================================================================

/// Детектор пересечений двух линий или линии и уровня
#[derive(Debug, Clone)]
pub struct CrossoverDetector {
    prev_a: Option<f64>,
    prev_b: Option<f64>,
    use_level: bool,
    level: f64,
}

impl CrossoverDetector {
    /// Создать детектор пересечения двух линий
    pub fn new() -> Self {
        Self {
            prev_a: None,
            prev_b: None,
            use_level: false,
            level: 0.0,
        }
    }

    /// Создать детектор пересечения линии с уровнем
    pub fn with_level(level: f64) -> Self {
        Self {
            prev_a: None,
            prev_b: None,
            use_level: true,
            level,
        }
    }

    /// Обновить и проверить пересечение двух линий
    pub fn update(&mut self, a: f64, b: f64) -> Option<SignalKind> {
        let result = if let (Some(pa), Some(pb)) = (self.prev_a, self.prev_b) {
            if CrossoverType::CrossUp.check(pa, a, pb, b) {
                Some(SignalKind::CrossoverUp)
            } else if CrossoverType::CrossDown.check(pa, a, pb, b) {
                Some(SignalKind::CrossoverDown)
            } else {
                None
            }
        } else {
            None
        };

        self.prev_a = Some(a);
        self.prev_b = Some(b);
        result
    }

    /// Обновить и проверить пересечение с уровнем
    pub fn update_level(&mut self, value: f64) -> Option<SignalKind> {
        let result = if let Some(prev) = self.prev_a {
            if CrossoverType::CrossUp.check_level(prev, value, self.level) {
                Some(SignalKind::CrossoverUp)
            } else if CrossoverType::CrossDown.check_level(prev, value, self.level) {
                Some(SignalKind::CrossoverDown)
            } else {
                None
            }
        } else {
            None
        };

        self.prev_a = Some(value);
        result
    }

    /// Сбросить состояние
    pub fn reset(&mut self) {
        self.prev_a = None;
        self.prev_b = None;
    }
}

impl Default for CrossoverDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// THRESHOLD MONITOR - Монитор порогов
// ============================================================================

/// Монитор пороговых условий (overbought/oversold zones)
#[derive(Debug, Clone)]
pub struct ThresholdMonitor {
    upper: f64,
    lower: f64,
    prev_value: Option<f64>,
    in_upper_zone: bool,
    in_lower_zone: bool,
}

impl ThresholdMonitor {
    /// Создать монитор с зонами (напр. 70/30 для RSI)
    pub fn new(upper: f64, lower: f64) -> Self {
        Self {
            upper,
            lower,
            prev_value: None,
            in_upper_zone: false,
            in_lower_zone: false,
        }
    }

    /// Обновить и проверить вход/выход из зон
    pub fn update(&mut self, value: f64) -> Option<SignalKind> {
        let result = if let Some(_prev) = self.prev_value {
            // Вход в верхнюю зону
            if !self.in_upper_zone && value > self.upper {
                self.in_upper_zone = true;
                Some(SignalKind::OscillatorOverbought)
            }
            // Выход из верхней зоны
            else if self.in_upper_zone && value < self.upper {
                self.in_upper_zone = false;
                Some(SignalKind::OscillatorExitOverbought)
            }
            // Вход в нижнюю зону
            else if !self.in_lower_zone && value < self.lower {
                self.in_lower_zone = true;
                Some(SignalKind::OscillatorOversold)
            }
            // Выход из нижней зоны
            else if self.in_lower_zone && value > self.lower {
                self.in_lower_zone = false;
                Some(SignalKind::OscillatorExitOversold)
            } else {
                None
            }
        } else {
            // Инициализация состояния
            self.in_upper_zone = value > self.upper;
            self.in_lower_zone = value < self.lower;
            None
        };

        self.prev_value = Some(value);
        result
    }

    /// Текущее состояние
    pub fn is_overbought(&self) -> bool {
        self.in_upper_zone
    }

    pub fn is_oversold(&self) -> bool {
        self.in_lower_zone
    }

    /// Сбросить состояние
    pub fn reset(&mut self) {
        self.prev_value = None;
        self.in_upper_zone = false;
        self.in_lower_zone = false;
    }
}

// ============================================================================
// ZERO CROSS DETECTOR - Детектор пересечения нуля
// ============================================================================

/// Детектор пересечения нулевой линии (для MACD, CCI, etc.)
#[derive(Debug, Clone)]
pub struct ZeroCrossDetector {
    prev_value: Option<f64>,
    tolerance: f64,
}

impl ZeroCrossDetector {
    pub fn new() -> Self {
        Self {
            prev_value: None,
            tolerance: 0.0,
        }
    }

    pub fn with_tolerance(tolerance: f64) -> Self {
        Self {
            prev_value: None,
            tolerance,
        }
    }

    pub fn update(&mut self, value: f64) -> Option<SignalKind> {
        let result = if let Some(prev) = self.prev_value {
            if prev <= self.tolerance && value > self.tolerance {
                Some(SignalKind::OscillatorZeroCrossUp)
            } else if prev >= -self.tolerance && value < -self.tolerance {
                Some(SignalKind::OscillatorZeroCrossDown)
            } else {
                None
            }
        } else {
            None
        };

        self.prev_value = Some(value);
        result
    }

    pub fn reset(&mut self) {
        self.prev_value = None;
    }
}

impl Default for ZeroCrossDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HISTOGRAM DETECTOR - Детектор гистограммы
// ============================================================================

/// Детектор изменений гистограммы
#[derive(Debug, Clone)]
pub struct HistogramDetector {
    prev_value: Option<f64>,
    prev_prev_value: Option<f64>,
}

impl HistogramDetector {
    pub fn new() -> Self {
        Self {
            prev_value: None,
            prev_prev_value: None,
        }
    }

    pub fn update(&mut self, value: f64) -> Option<SignalKind> {
        let result = if let Some(prev) = self.prev_value {
            // Смена знака
            if prev <= 0.0 && value > 0.0 {
                Some(SignalKind::HistogramPositive)
            } else if prev >= 0.0 && value < 0.0 {
                Some(SignalKind::HistogramNegative)
            }
            // Изменение направления
            else if let Some(prev_prev) = self.prev_prev_value {
                let prev_diff = prev - prev_prev;
                let curr_diff = value - prev;

                if prev_diff < 0.0 && curr_diff > 0.0 && value > 0.0 {
                    Some(SignalKind::HistogramGrowing)
                } else if prev_diff > 0.0 && curr_diff < 0.0 && value < 0.0 {
                    Some(SignalKind::HistogramShrinking)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.prev_prev_value = self.prev_value;
        self.prev_value = Some(value);
        result
    }

    pub fn reset(&mut self) {
        self.prev_value = None;
        self.prev_prev_value = None;
    }
}

impl Default for HistogramDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CHANNEL DETECTOR - Детектор канала
// ============================================================================

/// Детектор позиции в канале (Bollinger Bands, Keltner, etc.)
#[derive(Debug, Clone)]
pub struct ChannelDetector {
    prev_position: Option<ChannelPosition>,
    tolerance: f64,
}

impl ChannelDetector {
    pub fn new(tolerance: f64) -> Self {
        Self {
            prev_position: None,
            tolerance,
        }
    }

    pub fn update(&mut self, value: f64, upper: f64, lower: f64) -> Option<SignalKind> {
        let position = ChannelPosition::determine(value, upper, lower, self.tolerance);

        let result = if let Some(prev_pos) = self.prev_position {
            match (prev_pos, position) {
                // Касание границ
                (ChannelPosition::UpperHalf, ChannelPosition::AtUpper) => {
                    Some(SignalKind::ChannelUpperTouch)
                }
                (ChannelPosition::LowerHalf, ChannelPosition::AtLower) => {
                    Some(SignalKind::ChannelLowerTouch)
                }
                // Пробой границ
                (ChannelPosition::AtUpper | ChannelPosition::UpperHalf, ChannelPosition::AboveUpper) => {
                    Some(SignalKind::ChannelUpperBreak)
                }
                (ChannelPosition::AtLower | ChannelPosition::LowerHalf, ChannelPosition::BelowLower) => {
                    Some(SignalKind::ChannelLowerBreak)
                }
                // Возврат в канал
                (ChannelPosition::AboveUpper, ChannelPosition::AtUpper | ChannelPosition::UpperHalf) => {
                    Some(SignalKind::ChannelReenterFromAbove)
                }
                (ChannelPosition::BelowLower, ChannelPosition::AtLower | ChannelPosition::LowerHalf) => {
                    Some(SignalKind::ChannelReenterFromBelow)
                }
                // Пересечение середины
                (ChannelPosition::LowerHalf | ChannelPosition::AtLower, ChannelPosition::UpperHalf) => {
                    Some(SignalKind::ChannelMidCrossUp)
                }
                (ChannelPosition::UpperHalf | ChannelPosition::AtUpper, ChannelPosition::LowerHalf) => {
                    Some(SignalKind::ChannelMidCrossDown)
                }
                _ => None,
            }
        } else {
            None
        };

        self.prev_position = Some(position);
        result
    }

    pub fn current_position(&self) -> Option<ChannelPosition> {
        self.prev_position
    }

    pub fn reset(&mut self) {
        self.prev_position = None;
    }
}

// ============================================================================
// DIVERGENCE DETECTOR - Детектор дивергенций
// ============================================================================

/// Точка экстремума для детектора дивергенций
#[derive(Debug, Clone, Copy)]
struct ExtremumPoint {
    bar_index: usize,
    price: f64,
    indicator: f64,
    is_high: bool,
}

/// Детектор дивергенций между ценой и индикатором
#[derive(Debug, Clone)]
pub struct DivergenceDetector {
    /// Буфер экстремумов (highs и lows)
    extremums: ArrayVec<ExtremumPoint, 32>,
    /// Минимальное расстояние между экстремумами
    min_distance: usize,
    /// Lookback для поиска дивергенций
    lookback: usize,
    /// Текущий индекс бара
    bar_index: usize,
    /// Предыдущие значения для определения экстремумов
    prev_price: Option<f64>,
    prev_prev_price: Option<f64>,
    prev_indicator: Option<f64>,
    prev_prev_indicator: Option<f64>,
}

impl DivergenceDetector {
    pub fn new(min_distance: usize, lookback: usize) -> Self {
        Self {
            extremums: ArrayVec::new(),
            min_distance: min_distance.max(2),
            lookback: lookback.max(10),
            bar_index: 0,
            prev_price: None,
            prev_prev_price: None,
            prev_indicator: None,
            prev_prev_indicator: None,
        }
    }

    pub fn update(&mut self, price: f64, indicator: f64) -> Option<SignalKind> {
        let mut result = None;

        // Проверяем экстремум индикатора (используем prev как потенциальный экстремум)
        if let (Some(prev_prev_ind), Some(prev_ind)) = (self.prev_prev_indicator, self.prev_indicator) {
            if let (Some(_prev_prev_price), Some(prev_price)) = (self.prev_prev_price, self.prev_price) {
                // Локальный максимум индикатора
                if prev_ind > prev_prev_ind && prev_ind > indicator {
                    let point = ExtremumPoint {
                        bar_index: self.bar_index - 1,
                        price: prev_price,
                        indicator: prev_ind,
                        is_high: true,
                    };

                    // Ищем предыдущий максимум для сравнения
                    if let Some(prev_high) = self.find_previous_extremum(true) {
                        if self.bar_index - 1 - prev_high.bar_index >= self.min_distance {
                            // Медвежья дивергенция: цена выше, индикатор ниже
                            if DivergenceType::Bearish.check(
                                prev_high.price, point.price,
                                prev_high.indicator, point.indicator
                            ) {
                                result = Some(SignalKind::BearishDivergence);
                            }
                            // Скрытая медвежья: цена ниже, индикатор выше
                            else if DivergenceType::HiddenBearish.check(
                                prev_high.price, point.price,
                                prev_high.indicator, point.indicator
                            ) {
                                result = Some(SignalKind::HiddenBearishDivergence);
                            }
                        }
                    }

                    self.add_extremum(point);
                }
                // Локальный минимум индикатора
                else if prev_ind < prev_prev_ind && prev_ind < indicator {
                    let point = ExtremumPoint {
                        bar_index: self.bar_index - 1,
                        price: prev_price,
                        indicator: prev_ind,
                        is_high: false,
                    };

                    // Ищем предыдущий минимум для сравнения
                    if let Some(prev_low) = self.find_previous_extremum(false) {
                        if self.bar_index - 1 - prev_low.bar_index >= self.min_distance {
                            // Бычья дивергенция: цена ниже, индикатор выше
                            if DivergenceType::Bullish.check(
                                prev_low.price, point.price,
                                prev_low.indicator, point.indicator
                            ) {
                                result = Some(SignalKind::BullishDivergence);
                            }
                            // Скрытая бычья: цена выше, индикатор ниже
                            else if DivergenceType::HiddenBullish.check(
                                prev_low.price, point.price,
                                prev_low.indicator, point.indicator
                            ) {
                                result = Some(SignalKind::HiddenBullishDivergence);
                            }
                        }
                    }

                    self.add_extremum(point);
                }
            }
        }

        // Сдвигаем значения
        self.prev_prev_price = self.prev_price;
        self.prev_price = Some(price);
        self.prev_prev_indicator = self.prev_indicator;
        self.prev_indicator = Some(indicator);
        self.bar_index += 1;

        // Очищаем старые экстремумы
        self.cleanup_old_extremums();

        result
    }

    fn find_previous_extremum(&self, is_high: bool) -> Option<&ExtremumPoint> {
        self.extremums.iter().rev()
            .find(|e| e.is_high == is_high)
    }

    fn add_extremum(&mut self, point: ExtremumPoint) {
        if self.extremums.is_full() {
            self.extremums.remove(0);
        }
        self.extremums.push(point);
    }

    fn cleanup_old_extremums(&mut self) {
        let cutoff = self.bar_index.saturating_sub(self.lookback);
        self.extremums.retain(|e| e.bar_index >= cutoff);
    }

    pub fn reset(&mut self) {
        self.extremums.clear();
        self.bar_index = 0;
        self.prev_price = None;
        self.prev_prev_price = None;
        self.prev_indicator = None;
        self.prev_prev_indicator = None;
    }
}

// ============================================================================
// TREND DETECTOR - Детектор тренда
// ============================================================================

/// Детектор трендовых сигналов (Golden Cross, Death Cross, etc.)
#[derive(Debug, Clone)]
pub struct TrendDetector {
    fast_prev: Option<f64>,
    slow_prev: Option<f64>,
    price_above_fast: bool,
    price_above_slow: bool,
}

impl TrendDetector {
    pub fn new() -> Self {
        Self {
            fast_prev: None,
            slow_prev: None,
            price_above_fast: false,
            price_above_slow: false,
        }
    }

    /// Обновить с быстрой и медленной MA
    pub fn update(&mut self, price: f64, fast_ma: f64, slow_ma: f64) -> Option<SignalKind> {
        let result = if let (Some(fp), Some(sp)) = (self.fast_prev, self.slow_prev) {
            // Golden Cross: fast пересекает slow снизу вверх
            if fp <= sp && fast_ma > slow_ma {
                Some(SignalKind::GoldenCross)
            }
            // Death Cross: fast пересекает slow сверху вниз
            else if fp >= sp && fast_ma < slow_ma {
                Some(SignalKind::DeathCross)
            }
            // Цена пересекает тренд
            else if !self.price_above_fast && price > fast_ma {
                self.price_above_fast = true;
                Some(SignalKind::AboveTrend)
            } else if self.price_above_fast && price < fast_ma {
                self.price_above_fast = false;
                Some(SignalKind::BelowTrend)
            } else {
                None
            }
        } else {
            // Инициализация
            self.price_above_fast = price > fast_ma;
            self.price_above_slow = price > slow_ma;
            None
        };

        self.fast_prev = Some(fast_ma);
        self.slow_prev = Some(slow_ma);
        result
    }

    pub fn reset(&mut self) {
        self.fast_prev = None;
        self.slow_prev = None;
        self.price_above_fast = false;
        self.price_above_slow = false;
    }
}

impl Default for TrendDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VOLATILITY DETECTOR - Детектор волатильности
// ============================================================================

/// Детектор изменений волатильности
#[derive(Debug, Clone)]
pub struct VolatilityDetector {
    prev_regime: Option<VolatilityRegime>,
    prev_value: Option<f64>,
    /// Среднее значение для нормализации
    mean: f64,
    /// Стандартное отклонение для нормализации
    std: f64,
    /// Порог для squeeze (в стандартных отклонениях)
    squeeze_threshold: f64,
}

impl VolatilityDetector {
    pub fn new(mean: f64, std: f64, squeeze_threshold: f64) -> Self {
        Self {
            prev_regime: None,
            prev_value: None,
            mean,
            std,
            squeeze_threshold,
        }
    }

    pub fn update(&mut self, volatility: f64) -> Option<SignalKind> {
        let zscore = if self.std > 0.0 {
            (volatility - self.mean) / self.std
        } else {
            0.0
        };

        let regime = VolatilityRegime::from_zscore(zscore);

        let result = if let Some(prev_regime) = self.prev_regime {
            // Изменение режима
            match (prev_regime, regime) {
                (VolatilityRegime::VeryLow | VolatilityRegime::Low, VolatilityRegime::High | VolatilityRegime::VeryHigh) => {
                    Some(SignalKind::VolatilityBreakout)
                }
                (VolatilityRegime::Normal | VolatilityRegime::High, VolatilityRegime::VeryLow) => {
                    Some(SignalKind::VolatilityExtremeLow)
                }
                (VolatilityRegime::Normal | VolatilityRegime::Low, VolatilityRegime::VeryHigh) => {
                    Some(SignalKind::VolatilityExtremeHigh)
                }
                _ if zscore < -self.squeeze_threshold => {
                    Some(SignalKind::ChannelSqueeze)
                }
                _ => {
                    // Check for volatility increase/decrease
                    if let Some(pv) = self.prev_value {
                        if volatility > pv * 1.2 {
                            Some(SignalKind::VolatilityIncrease)
                        } else if volatility < pv * 0.8 {
                            Some(SignalKind::VolatilityDecrease)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            }
        } else {
            None
        };

        self.prev_regime = Some(regime);
        self.prev_value = Some(volatility);
        result
    }

    /// Обновить статистику для нормализации
    pub fn update_stats(&mut self, mean: f64, std: f64) {
        self.mean = mean;
        self.std = std;
    }

    pub fn reset(&mut self) {
        self.prev_regime = None;
        self.prev_value = None;
    }
}

// ============================================================================
// VOLUME DETECTOR - Детектор объёма
// ============================================================================

/// Детектор объёмных сигналов
#[derive(Debug, Clone)]
pub struct VolumeDetector {
    avg_volume: f64,
    prev_character: Option<VolumeCharacter>,
    prev_delta: Option<f64>,
}

impl VolumeDetector {
    pub fn new(avg_volume: f64) -> Self {
        Self {
            avg_volume,
            prev_character: None,
            prev_delta: None,
        }
    }

    pub fn update(&mut self, volume: f64, delta: Option<f64>) -> Option<SignalKind> {
        let ratio = if self.avg_volume > 0.0 {
            volume / self.avg_volume
        } else {
            1.0
        };

        let character = VolumeCharacter::from_ratio(ratio);

        let result = match character {
            VolumeCharacter::Spike => Some(SignalKind::VolumeSpike),
            VolumeCharacter::Climax => Some(SignalKind::VolumeClimax),
            VolumeCharacter::AboveAverage | VolumeCharacter::High => {
                Some(SignalKind::VolumeAboveAverage)
            }
            VolumeCharacter::VeryLow => Some(SignalKind::VolumeBelowAverage),
            _ => None,
        };

        // Проверка delta
        let delta_signal = if let (Some(d), Some(prev_d)) = (delta, self.prev_delta) {
            if d > 0.0 && prev_d <= 0.0 {
                Some(SignalKind::VolumeDeltaPositive)
            } else if d < 0.0 && prev_d >= 0.0 {
                Some(SignalKind::VolumeDeltaNegative)
            } else {
                None
            }
        } else {
            None
        };

        self.prev_character = Some(character);
        self.prev_delta = delta;

        // Приоритет: delta_signal, затем volume signal
        delta_signal.or(result)
    }

    pub fn update_avg(&mut self, avg_volume: f64) {
        self.avg_volume = avg_volume;
    }

    pub fn reset(&mut self) {
        self.prev_character = None;
        self.prev_delta = None;
    }
}

// ============================================================================
// SWING DETECTOR - Детектор свингов (фракталов)
// ============================================================================

/// Детектор свинг-точек (локальных экстремумов)
#[derive(Debug, Clone)]
pub struct SwingDetector {
    /// Количество баров с каждой стороны для подтверждения свинга
    lookback: usize,
    /// Буфер цен
    highs: ArrayVec<f64, 64>,
    lows: ArrayVec<f64, 64>,
    bar_index: usize,
}

impl SwingDetector {
    pub fn new(lookback: usize) -> Self {
        Self {
            lookback: lookback.max(1).min(30),
            highs: ArrayVec::new(),
            lows: ArrayVec::new(),
            bar_index: 0,
        }
    }

    pub fn update(&mut self, high: f64, low: f64) -> Option<SignalKind> {
        // Добавляем в буфер
        if self.highs.len() >= self.lookback * 2 + 1 {
            self.highs.remove(0);
            self.lows.remove(0);
        }
        self.highs.push(high);
        self.lows.push(low);
        self.bar_index += 1;

        // Проверяем только когда буфер заполнен
        if self.highs.len() < self.lookback * 2 + 1 {
            return None;
        }

        let mid = self.lookback;
        let mid_high = self.highs[mid];
        let mid_low = self.lows[mid];

        // Swing High: все highs слева и справа ниже mid_high
        let is_swing_high = self.highs[..mid].iter().all(|&h| h < mid_high)
            && self.highs[mid + 1..].iter().all(|&h| h < mid_high);

        // Swing Low: все lows слева и справа выше mid_low
        let is_swing_low = self.lows[..mid].iter().all(|&l| l > mid_low)
            && self.lows[mid + 1..].iter().all(|&l| l > mid_low);

        if is_swing_high {
            Some(SignalKind::SwingHighConfirmed)
        } else if is_swing_low {
            Some(SignalKind::SwingLowConfirmed)
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.bar_index = 0;
    }
}

// ============================================================================
// MULTI-SIGNAL DETECTOR - Композитный детектор
// ============================================================================

/// Агрегатор нескольких сигналов
#[derive(Debug, Clone)]
pub struct MultiSignalDetector {
    signals: ArrayVec<SignalKind, 16>,
    bar_index: usize,
}

impl MultiSignalDetector {
    pub fn new() -> Self {
        Self {
            signals: ArrayVec::new(),
            bar_index: 0,
        }
    }

    /// Добавить сигнал
    pub fn add_signal(&mut self, signal: SignalKind) {
        if !self.signals.is_full() {
            self.signals.push(signal);
        }
    }

    /// Очистить сигналы текущего бара
    pub fn clear(&mut self) {
        self.signals.clear();
    }

    /// Обработать накопленные сигналы и определить итоговый
    pub fn evaluate(&self) -> Option<SignalKind> {
        if self.signals.is_empty() {
            return None;
        }

        // Подсчёт bullish/bearish сигналов
        let bullish_count = self.signals.iter().filter(|s| s.direction() > 0).count();
        let bearish_count = self.signals.iter().filter(|s| s.direction() < 0).count();

        // Множественное подтверждение
        if bullish_count >= 3 && bearish_count == 0 {
            Some(SignalKind::StrongBullish)
        } else if bearish_count >= 3 && bullish_count == 0 {
            Some(SignalKind::StrongBearish)
        } else if bullish_count >= 2 && bearish_count == 0 {
            Some(SignalKind::MultipleConfirmation)
        } else if bearish_count >= 2 && bullish_count == 0 {
            Some(SignalKind::MultipleConfirmation)
        } else if bullish_count > 0 && bearish_count > 0 {
            Some(SignalKind::SignalConflict)
        } else if self.signals.len() == 1 {
            Some(self.signals[0])
        } else {
            None
        }
    }

    /// Получить все сигналы
    pub fn signals(&self) -> &[SignalKind] {
        &self.signals
    }

    /// Advance to next bar
    pub fn next_bar(&mut self) {
        self.bar_index += 1;
        self.signals.clear();
    }
}

impl Default for MultiSignalDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossover_detector() {
        let mut detector = CrossoverDetector::new();

        // No signal on first update
        assert!(detector.update(45.0, 50.0).is_none());

        // Cross up
        let signal = detector.update(55.0, 50.0);
        assert_eq!(signal, Some(SignalKind::CrossoverUp));

        // No cross
        assert!(detector.update(56.0, 50.0).is_none());

        // Cross down
        let signal = detector.update(48.0, 50.0);
        assert_eq!(signal, Some(SignalKind::CrossoverDown));
    }

    #[test]
    fn test_threshold_monitor() {
        let mut monitor = ThresholdMonitor::new(70.0, 30.0);

        // Initial update
        assert!(monitor.update(50.0).is_none());

        // Enter overbought
        let signal = monitor.update(75.0);
        assert_eq!(signal, Some(SignalKind::OscillatorOverbought));
        assert!(monitor.is_overbought());

        // Exit overbought
        let signal = monitor.update(65.0);
        assert_eq!(signal, Some(SignalKind::OscillatorExitOverbought));
        assert!(!monitor.is_overbought());

        // Enter oversold
        assert!(monitor.update(35.0).is_none());
        let signal = monitor.update(25.0);
        assert_eq!(signal, Some(SignalKind::OscillatorOversold));
        assert!(monitor.is_oversold());
    }

    #[test]
    fn test_zero_cross_detector() {
        let mut detector = ZeroCrossDetector::new();

        assert!(detector.update(-5.0).is_none());

        let signal = detector.update(5.0);
        assert_eq!(signal, Some(SignalKind::OscillatorZeroCrossUp));

        let signal = detector.update(-5.0);
        assert_eq!(signal, Some(SignalKind::OscillatorZeroCrossDown));
    }

    #[test]
    fn test_histogram_detector() {
        let mut detector = HistogramDetector::new();

        assert!(detector.update(-5.0).is_none());
        assert!(detector.update(-3.0).is_none());

        let signal = detector.update(2.0);
        assert_eq!(signal, Some(SignalKind::HistogramPositive));

        assert!(detector.update(5.0).is_none());

        let signal = detector.update(-1.0);
        assert_eq!(signal, Some(SignalKind::HistogramNegative));
    }

    #[test]
    fn test_trend_detector() {
        let mut detector = TrendDetector::new();

        // Initial
        assert!(detector.update(100.0, 98.0, 100.0).is_none());

        // Golden Cross
        let signal = detector.update(102.0, 101.0, 99.0);
        assert_eq!(signal, Some(SignalKind::GoldenCross));

        // Death Cross
        let signal = detector.update(95.0, 96.0, 98.0);
        assert_eq!(signal, Some(SignalKind::DeathCross));
    }

    #[test]
    fn test_swing_detector() {
        let mut detector = SwingDetector::new(2);

        // Build up buffer: 5 bars needed (2 left + 1 mid + 2 right)
        // Pattern: low - higher - peak - lower - low
        assert!(detector.update(100.0, 95.0).is_none());
        assert!(detector.update(105.0, 98.0).is_none());
        assert!(detector.update(110.0, 100.0).is_none()); // potential swing high
        assert!(detector.update(105.0, 98.0).is_none());

        let signal = detector.update(100.0, 95.0);
        assert_eq!(signal, Some(SignalKind::SwingHighConfirmed));
    }

    #[test]
    fn test_multi_signal_detector() {
        let mut detector = MultiSignalDetector::new();

        detector.add_signal(SignalKind::CrossoverUp);
        detector.add_signal(SignalKind::OscillatorExitOversold);

        let signal = detector.evaluate();
        assert_eq!(signal, Some(SignalKind::MultipleConfirmation));

        detector.clear();
        detector.add_signal(SignalKind::CrossoverUp);
        detector.add_signal(SignalKind::CrossoverDown);

        let signal = detector.evaluate();
        assert_eq!(signal, Some(SignalKind::SignalConflict));
    }
}
