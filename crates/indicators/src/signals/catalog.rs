//! Signal Catalog - полный каталог всех типов сигналов
//!
//! Сигналы организованы по категориям:
//! - Basic: базовые сигналы (crossover, threshold, etc.)
//! - Trend: трендовые сигналы
//! - Momentum: импульсные сигналы
//! - Volatility: волатильность
//! - Volume: объёмные сигналы
//! - Pattern: паттерны
//! - Composite: комбинированные сигналы
//! - Structure: структурные сигналы

use serde::{Deserialize, Serialize};

// ============================================================================
// SIGNAL KIND - ЧТО ИМЕННО ПРОИЗОШЛО
// ============================================================================

/// Полный каталог типов сигналов
///
/// Каждый сигнал - это абстракция, не привязанная к конкретному индикатору.
/// Например, `OscillatorOverbought` может применяться к RSI, Stochastic, CCI и т.д.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalKind {
    // ========================================================================
    // BASIC - Базовые сигналы
    // ========================================================================

    /// Пересечение снизу вверх (bullish crossover)
    CrossoverUp,
    /// Пересечение сверху вниз (bearish crossover)
    CrossoverDown,

    /// Пробой уровня вверх
    BreakoutUp,
    /// Пробой уровня вниз
    BreakoutDown,

    /// Отскок от уровня вверх (bounce from support)
    BounceUp,
    /// Отскок от уровня вниз (bounce from resistance)
    BounceDown,

    /// Касание уровня (touch without break)
    TouchLevel,

    /// Вход в зону (entered zone)
    EnteredZone,
    /// Выход из зоны (exited zone)
    ExitedZone,

    // ========================================================================
    // TREND - Трендовые сигналы
    // ========================================================================

    /// Начало восходящего тренда
    TrendStartUp,
    /// Начало нисходящего тренда
    TrendStartDown,

    /// Усиление тренда
    TrendStrengthening,
    /// Ослабление тренда
    TrendWeakening,

    /// Разворот тренда вверх
    TrendReversalUp,
    /// Разворот тренда вниз
    TrendReversalDown,

    /// Продолжение тренда после консолидации
    TrendContinuation,

    /// Цена выше трендовой линии/MA
    AboveTrend,
    /// Цена ниже трендовой линии/MA
    BelowTrend,

    /// Золотой крест (fast MA > slow MA)
    GoldenCross,
    /// Смертельный крест (fast MA < slow MA)
    DeathCross,

    // ========================================================================
    // MOMENTUM / OSCILLATOR - Импульсные сигналы
    // ========================================================================

    /// Осциллятор вошёл в зону перекупленности
    OscillatorOverbought,
    /// Осциллятор вошёл в зону перепроданности
    OscillatorOversold,

    /// Осциллятор вышел из зоны перекупленности (sell signal)
    OscillatorExitOverbought,
    /// Осциллятор вышел из зоны перепроданности (buy signal)
    OscillatorExitOversold,

    /// Осциллятор пересёк нулевую линию вверх
    OscillatorZeroCrossUp,
    /// Осциллятор пересёк нулевую линию вниз
    OscillatorZeroCrossDown,

    /// Осциллятор достиг экстремума (peak/trough)
    OscillatorExtreme,

    /// Гистограмма сменила знак на положительный
    HistogramPositive,
    /// Гистограмма сменила знак на отрицательный
    HistogramNegative,

    /// Гистограмма растёт (momentum increasing)
    HistogramGrowing,
    /// Гистограмма падает (momentum decreasing)
    HistogramShrinking,

    /// Двойной осциллятор: оба перекуплены
    DualOscillatorOverbought,
    /// Двойной осциллятор: оба перепроданы
    DualOscillatorOversold,
    /// Двойной осциллятор: синхронный сигнал
    DualOscillatorSync,

    // ========================================================================
    // DIVERGENCE - Дивергенции
    // ========================================================================

    /// Бычья дивергенция (цена падает, индикатор растёт)
    BullishDivergence,
    /// Медвежья дивергенция (цена растёт, индикатор падает)
    BearishDivergence,

    /// Скрытая бычья дивергенция (продолжение тренда)
    HiddenBullishDivergence,
    /// Скрытая медвежья дивергенция (продолжение тренда)
    HiddenBearishDivergence,

    /// Множественная дивергенция (несколько индикаторов)
    MultipleDivergence,

    // ========================================================================
    // CHANNEL / BAND - Канальные сигналы
    // ========================================================================

    /// Цена коснулась верхней границы канала
    ChannelUpperTouch,
    /// Цена коснулась нижней границы канала
    ChannelLowerTouch,

    /// Цена вышла за верхнюю границу канала
    ChannelUpperBreak,
    /// Цена вышла за нижнюю границу канала
    ChannelLowerBreak,

    /// Цена вернулась в канал сверху
    ChannelReenterFromAbove,
    /// Цена вернулась в канал снизу
    ChannelReenterFromBelow,

    /// Сужение канала (squeeze)
    ChannelSqueeze,
    /// Расширение канала (expansion)
    ChannelExpansion,

    /// Цена в верхней половине канала
    ChannelUpperHalf,
    /// Цена в нижней половине канала
    ChannelLowerHalf,

    /// Цена пересекла середину канала вверх
    ChannelMidCrossUp,
    /// Цена пересекла середину канала вниз
    ChannelMidCrossDown,

    // ========================================================================
    // VOLATILITY - Волатильность
    // ========================================================================

    /// Волатильность выросла (ATR expansion, BB widening)
    VolatilityIncrease,
    /// Волатильность упала (ATR contraction, BB narrowing)
    VolatilityDecrease,

    /// Экстремально низкая волатильность (потенциальный breakout)
    VolatilityExtremeLow,
    /// Экстремально высокая волатильность
    VolatilityExtremeHigh,

    /// Волатильность пробила исторический уровень
    VolatilityBreakout,

    /// Начало волатильного периода
    VolatilityRegimeHigh,
    /// Начало спокойного периода
    VolatilityRegimeLow,

    // ========================================================================
    // VOLUME - Объёмные сигналы
    // ========================================================================

    /// Всплеск объёма (volume spike)
    VolumeSpike,
    /// Объём выше среднего
    VolumeAboveAverage,
    /// Объём ниже среднего (low interest)
    VolumeBelowAverage,

    /// Объём подтверждает движение (volume confirmation)
    VolumeConfirmation,
    /// Объём не подтверждает движение (volume divergence)
    VolumeDivergence,

    /// Кульминация объёма (climax)
    VolumeClimax,
    /// Иссякание объёма (exhaustion)
    VolumeExhaustion,

    /// Накопление (accumulation)
    VolumeAccumulation,
    /// Распределение (distribution)
    VolumeDistribution,

    /// Delta положительная (больше покупок)
    VolumeDeltaPositive,
    /// Delta отрицательная (больше продаж)
    VolumeDeltaNegative,

    // ========================================================================
    // PATTERN - Паттерны
    // ========================================================================

    /// Свечной паттерн: бычий
    CandlePatternBullish,
    /// Свечной паттерн: медвежий
    CandlePatternBearish,
    /// Свечной паттерн: разворотный
    CandlePatternReversal,
    /// Свечной паттерн: продолжение
    CandlePatternContinuation,

    /// Дожи (нерешительность)
    CandleDoji,
    /// Молот/повешенный
    CandleHammer,
    /// Поглощение
    CandleEngulfing,

    /// Фрактал вверх (локальный максимум)
    FractalHigh,
    /// Фрактал вниз (локальный минимум)
    FractalLow,

    /// Swing high подтверждён
    SwingHighConfirmed,
    /// Swing low подтверждён
    SwingLowConfirmed,

    /// Формируется паттерн (в процессе)
    PatternForming,
    /// Паттерн завершён
    PatternComplete,
    /// Паттерн сломан/отменён
    PatternBroken,

    // ========================================================================
    // STRUCTURE - Рыночная структура
    // ========================================================================

    /// Break of Structure вверх (bullish BOS)
    BreakOfStructureUp,
    /// Break of Structure вниз (bearish BOS)
    BreakOfStructureDown,

    /// Change of Character (смена характера рынка)
    ChangeOfCharacter,

    /// Sweep ликвидности вверх
    LiquiditySweepHigh,
    /// Sweep ликвидности вниз
    LiquiditySweepLow,

    /// Fair Value Gap обнаружен
    FairValueGap,
    /// Fair Value Gap заполнен
    FairValueGapFilled,

    /// Order Block бычий
    OrderBlockBullish,
    /// Order Block медвежий
    OrderBlockBearish,

    /// Imbalance / неэффективность
    Imbalance,

    // ========================================================================
    // SUPPORT / RESISTANCE - Уровни
    // ========================================================================

    /// Приближение к сопротивлению
    ApproachingResistance,
    /// Приближение к поддержке
    ApproachingSupport,

    /// Пробой сопротивления
    ResistanceBreak,
    /// Пробой поддержки
    SupportBreak,

    /// Тест уровня (retest)
    LevelRetest,
    /// Уровень стал поддержкой (бывшее сопротивление)
    ResistanceTurnedSupport,
    /// Уровень стал сопротивлением (бывшая поддержка)
    SupportTurnedResistance,

    // ========================================================================
    // TIME / CYCLE - Временные сигналы
    // ========================================================================

    /// Начало торговой сессии
    SessionOpen,
    /// Конец торговой сессии
    SessionClose,

    /// Начало месяца/квартала
    PeriodStart,
    /// Конец месяца/квартала
    PeriodEnd,

    /// Цикл достиг максимума
    CyclePeak,
    /// Цикл достиг минимума
    CycleTrough,

    // ========================================================================
    // COMPOSITE - Комбинированные сигналы
    // ========================================================================

    /// Множественное подтверждение (несколько индикаторов согласны)
    MultipleConfirmation,
    /// Конфликт сигналов (индикаторы не согласны)
    SignalConflict,

    /// Сильный бычий сигнал (высокая уверенность)
    StrongBullish,
    /// Сильный медвежий сигнал (высокая уверенность)
    StrongBearish,

    /// Слабый/сомнительный сигнал
    WeakSignal,

    /// Подтверждение предыдущего сигнала
    Confirmation,
    /// Отмена предыдущего сигнала
    Invalidation,

    // ========================================================================
    // SPECIAL - Специальные сигналы
    // ========================================================================

    /// Аномалия/выброс в данных
    Anomaly,

    /// Статистически значимое событие
    StatisticallySignificant,

    /// Изменение режима рынка
    RegimeChange,

    /// Настраиваемый пользовательский сигнал
    Custom,
}

impl SignalKind {
    /// Получить направление сигнала (-1 = bearish, 0 = neutral, 1 = bullish)
    pub fn direction(&self) -> i8 {
        match self {
            // Bullish signals
            Self::CrossoverUp
            | Self::BreakoutUp
            | Self::BounceUp
            | Self::TrendStartUp
            | Self::TrendReversalUp
            | Self::GoldenCross
            | Self::OscillatorExitOversold
            | Self::OscillatorZeroCrossUp
            | Self::BullishDivergence
            | Self::HiddenBullishDivergence
            | Self::ChannelLowerTouch
            | Self::ChannelReenterFromBelow
            | Self::ChannelMidCrossUp
            | Self::VolumeDeltaPositive
            | Self::VolumeAccumulation
            | Self::CandlePatternBullish
            | Self::CandleHammer
            | Self::FractalLow
            | Self::SwingLowConfirmed
            | Self::BreakOfStructureUp
            | Self::LiquiditySweepLow
            | Self::OrderBlockBullish
            | Self::ResistanceBreak
            | Self::ResistanceTurnedSupport
            | Self::StrongBullish => 1,

            // Bearish signals
            Self::CrossoverDown
            | Self::BreakoutDown
            | Self::BounceDown
            | Self::TrendStartDown
            | Self::TrendReversalDown
            | Self::DeathCross
            | Self::OscillatorExitOverbought
            | Self::OscillatorZeroCrossDown
            | Self::BearishDivergence
            | Self::HiddenBearishDivergence
            | Self::ChannelUpperTouch
            | Self::ChannelReenterFromAbove
            | Self::ChannelMidCrossDown
            | Self::VolumeDeltaNegative
            | Self::VolumeDistribution
            | Self::CandlePatternBearish
            | Self::FractalHigh
            | Self::SwingHighConfirmed
            | Self::BreakOfStructureDown
            | Self::LiquiditySweepHigh
            | Self::OrderBlockBearish
            | Self::SupportBreak
            | Self::SupportTurnedResistance
            | Self::StrongBearish => -1,

            // Neutral signals
            _ => 0,
        }
    }

    /// Получить категорию сигнала
    pub fn category(&self) -> SignalCategory {
        match self {
            Self::CrossoverUp
            | Self::CrossoverDown
            | Self::BreakoutUp
            | Self::BreakoutDown
            | Self::BounceUp
            | Self::BounceDown
            | Self::TouchLevel
            | Self::EnteredZone
            | Self::ExitedZone => SignalCategory::Basic,

            Self::TrendStartUp
            | Self::TrendStartDown
            | Self::TrendStrengthening
            | Self::TrendWeakening
            | Self::TrendReversalUp
            | Self::TrendReversalDown
            | Self::TrendContinuation
            | Self::AboveTrend
            | Self::BelowTrend
            | Self::GoldenCross
            | Self::DeathCross => SignalCategory::Trend,

            Self::OscillatorOverbought
            | Self::OscillatorOversold
            | Self::OscillatorExitOverbought
            | Self::OscillatorExitOversold
            | Self::OscillatorZeroCrossUp
            | Self::OscillatorZeroCrossDown
            | Self::OscillatorExtreme
            | Self::HistogramPositive
            | Self::HistogramNegative
            | Self::HistogramGrowing
            | Self::HistogramShrinking
            | Self::DualOscillatorOverbought
            | Self::DualOscillatorOversold
            | Self::DualOscillatorSync => SignalCategory::Momentum,

            Self::BullishDivergence
            | Self::BearishDivergence
            | Self::HiddenBullishDivergence
            | Self::HiddenBearishDivergence
            | Self::MultipleDivergence => SignalCategory::Divergence,

            Self::ChannelUpperTouch
            | Self::ChannelLowerTouch
            | Self::ChannelUpperBreak
            | Self::ChannelLowerBreak
            | Self::ChannelReenterFromAbove
            | Self::ChannelReenterFromBelow
            | Self::ChannelSqueeze
            | Self::ChannelExpansion
            | Self::ChannelUpperHalf
            | Self::ChannelLowerHalf
            | Self::ChannelMidCrossUp
            | Self::ChannelMidCrossDown => SignalCategory::Channel,

            Self::VolatilityIncrease
            | Self::VolatilityDecrease
            | Self::VolatilityExtremeLow
            | Self::VolatilityExtremeHigh
            | Self::VolatilityBreakout
            | Self::VolatilityRegimeHigh
            | Self::VolatilityRegimeLow => SignalCategory::Volatility,

            Self::VolumeSpike
            | Self::VolumeAboveAverage
            | Self::VolumeBelowAverage
            | Self::VolumeConfirmation
            | Self::VolumeDivergence
            | Self::VolumeClimax
            | Self::VolumeExhaustion
            | Self::VolumeAccumulation
            | Self::VolumeDistribution
            | Self::VolumeDeltaPositive
            | Self::VolumeDeltaNegative => SignalCategory::Volume,

            Self::CandlePatternBullish
            | Self::CandlePatternBearish
            | Self::CandlePatternReversal
            | Self::CandlePatternContinuation
            | Self::CandleDoji
            | Self::CandleHammer
            | Self::CandleEngulfing
            | Self::FractalHigh
            | Self::FractalLow
            | Self::SwingHighConfirmed
            | Self::SwingLowConfirmed
            | Self::PatternForming
            | Self::PatternComplete
            | Self::PatternBroken => SignalCategory::Pattern,

            Self::BreakOfStructureUp
            | Self::BreakOfStructureDown
            | Self::ChangeOfCharacter
            | Self::LiquiditySweepHigh
            | Self::LiquiditySweepLow
            | Self::FairValueGap
            | Self::FairValueGapFilled
            | Self::OrderBlockBullish
            | Self::OrderBlockBearish
            | Self::Imbalance => SignalCategory::Structure,

            Self::ApproachingResistance
            | Self::ApproachingSupport
            | Self::ResistanceBreak
            | Self::SupportBreak
            | Self::LevelRetest
            | Self::ResistanceTurnedSupport
            | Self::SupportTurnedResistance => SignalCategory::Level,

            Self::SessionOpen
            | Self::SessionClose
            | Self::PeriodStart
            | Self::PeriodEnd
            | Self::CyclePeak
            | Self::CycleTrough => SignalCategory::Time,

            Self::MultipleConfirmation
            | Self::SignalConflict
            | Self::StrongBullish
            | Self::StrongBearish
            | Self::WeakSignal
            | Self::Confirmation
            | Self::Invalidation => SignalCategory::Composite,

            Self::Anomaly
            | Self::StatisticallySignificant
            | Self::RegimeChange
            | Self::Custom => SignalCategory::Special,
        }
    }

    /// Является ли сигнал торговым (actionable)
    pub fn is_actionable(&self) -> bool {
        matches!(
            self.direction(),
            1 | -1
        ) && !matches!(
            self,
            Self::PatternForming
            | Self::EnteredZone
            | Self::WeakSignal
            | Self::SignalConflict
        )
    }

    /// Получить строковое описание
    pub fn description(&self) -> &'static str {
        match self {
            Self::CrossoverUp => "Crossover Up",
            Self::CrossoverDown => "Crossover Down",
            Self::BreakoutUp => "Breakout Up",
            Self::BreakoutDown => "Breakout Down",
            Self::BounceUp => "Bounce Up",
            Self::BounceDown => "Bounce Down",
            Self::TouchLevel => "Touch Level",
            Self::EnteredZone => "Entered Zone",
            Self::ExitedZone => "Exited Zone",

            Self::TrendStartUp => "Uptrend Start",
            Self::TrendStartDown => "Downtrend Start",
            Self::TrendStrengthening => "Trend Strengthening",
            Self::TrendWeakening => "Trend Weakening",
            Self::TrendReversalUp => "Bullish Reversal",
            Self::TrendReversalDown => "Bearish Reversal",
            Self::TrendContinuation => "Trend Continuation",
            Self::AboveTrend => "Above Trend",
            Self::BelowTrend => "Below Trend",
            Self::GoldenCross => "Golden Cross",
            Self::DeathCross => "Death Cross",

            Self::OscillatorOverbought => "Overbought",
            Self::OscillatorOversold => "Oversold",
            Self::OscillatorExitOverbought => "Exit Overbought",
            Self::OscillatorExitOversold => "Exit Oversold",
            Self::OscillatorZeroCrossUp => "Zero Cross Up",
            Self::OscillatorZeroCrossDown => "Zero Cross Down",
            Self::OscillatorExtreme => "Extreme Reading",
            Self::HistogramPositive => "Histogram Positive",
            Self::HistogramNegative => "Histogram Negative",
            Self::HistogramGrowing => "Histogram Growing",
            Self::HistogramShrinking => "Histogram Shrinking",
            Self::DualOscillatorOverbought => "Dual Overbought",
            Self::DualOscillatorOversold => "Dual Oversold",
            Self::DualOscillatorSync => "Dual Sync",

            Self::BullishDivergence => "Bullish Divergence",
            Self::BearishDivergence => "Bearish Divergence",
            Self::HiddenBullishDivergence => "Hidden Bullish Div",
            Self::HiddenBearishDivergence => "Hidden Bearish Div",
            Self::MultipleDivergence => "Multiple Divergence",

            Self::ChannelUpperTouch => "Upper Band Touch",
            Self::ChannelLowerTouch => "Lower Band Touch",
            Self::ChannelUpperBreak => "Upper Band Break",
            Self::ChannelLowerBreak => "Lower Band Break",
            Self::ChannelReenterFromAbove => "Reenter from Above",
            Self::ChannelReenterFromBelow => "Reenter from Below",
            Self::ChannelSqueeze => "Channel Squeeze",
            Self::ChannelExpansion => "Channel Expansion",
            Self::ChannelUpperHalf => "Upper Half",
            Self::ChannelLowerHalf => "Lower Half",
            Self::ChannelMidCrossUp => "Mid Cross Up",
            Self::ChannelMidCrossDown => "Mid Cross Down",

            Self::VolatilityIncrease => "Vol Increase",
            Self::VolatilityDecrease => "Vol Decrease",
            Self::VolatilityExtremeLow => "Extreme Low Vol",
            Self::VolatilityExtremeHigh => "Extreme High Vol",
            Self::VolatilityBreakout => "Vol Breakout",
            Self::VolatilityRegimeHigh => "High Vol Regime",
            Self::VolatilityRegimeLow => "Low Vol Regime",

            Self::VolumeSpike => "Volume Spike",
            Self::VolumeAboveAverage => "Above Avg Volume",
            Self::VolumeBelowAverage => "Below Avg Volume",
            Self::VolumeConfirmation => "Volume Confirms",
            Self::VolumeDivergence => "Volume Divergence",
            Self::VolumeClimax => "Volume Climax",
            Self::VolumeExhaustion => "Volume Exhaustion",
            Self::VolumeAccumulation => "Accumulation",
            Self::VolumeDistribution => "Distribution",
            Self::VolumeDeltaPositive => "Positive Delta",
            Self::VolumeDeltaNegative => "Negative Delta",

            Self::CandlePatternBullish => "Bullish Candle",
            Self::CandlePatternBearish => "Bearish Candle",
            Self::CandlePatternReversal => "Reversal Candle",
            Self::CandlePatternContinuation => "Continuation",
            Self::CandleDoji => "Doji",
            Self::CandleHammer => "Hammer",
            Self::CandleEngulfing => "Engulfing",
            Self::FractalHigh => "Fractal High",
            Self::FractalLow => "Fractal Low",
            Self::SwingHighConfirmed => "Swing High",
            Self::SwingLowConfirmed => "Swing Low",
            Self::PatternForming => "Pattern Forming",
            Self::PatternComplete => "Pattern Complete",
            Self::PatternBroken => "Pattern Broken",

            Self::BreakOfStructureUp => "BOS Up",
            Self::BreakOfStructureDown => "BOS Down",
            Self::ChangeOfCharacter => "CHoCH",
            Self::LiquiditySweepHigh => "Sweep High",
            Self::LiquiditySweepLow => "Sweep Low",
            Self::FairValueGap => "FVG",
            Self::FairValueGapFilled => "FVG Filled",
            Self::OrderBlockBullish => "Bullish OB",
            Self::OrderBlockBearish => "Bearish OB",
            Self::Imbalance => "Imbalance",

            Self::ApproachingResistance => "Near Resistance",
            Self::ApproachingSupport => "Near Support",
            Self::ResistanceBreak => "Resistance Break",
            Self::SupportBreak => "Support Break",
            Self::LevelRetest => "Level Retest",
            Self::ResistanceTurnedSupport => "R→S Flip",
            Self::SupportTurnedResistance => "S→R Flip",

            Self::SessionOpen => "Session Open",
            Self::SessionClose => "Session Close",
            Self::PeriodStart => "Period Start",
            Self::PeriodEnd => "Period End",
            Self::CyclePeak => "Cycle Peak",
            Self::CycleTrough => "Cycle Trough",

            Self::MultipleConfirmation => "Confirmed",
            Self::SignalConflict => "Conflict",
            Self::StrongBullish => "Strong Bullish",
            Self::StrongBearish => "Strong Bearish",
            Self::WeakSignal => "Weak Signal",
            Self::Confirmation => "Confirmation",
            Self::Invalidation => "Invalidation",

            Self::Anomaly => "Anomaly",
            Self::StatisticallySignificant => "Significant",
            Self::RegimeChange => "Regime Change",
            Self::Custom => "Custom",
        }
    }
}

/// Категории сигналов
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalCategory {
    Basic,
    Trend,
    Momentum,
    Divergence,
    Channel,
    Volatility,
    Volume,
    Pattern,
    Structure,
    Level,
    Time,
    Composite,
    Special,
}

impl SignalCategory {
    pub fn all() -> &'static [SignalCategory] {
        &[
            Self::Basic,
            Self::Trend,
            Self::Momentum,
            Self::Divergence,
            Self::Channel,
            Self::Volatility,
            Self::Volume,
            Self::Pattern,
            Self::Structure,
            Self::Level,
            Self::Time,
            Self::Composite,
            Self::Special,
        ]
    }
}

// ============================================================================
// SIGNAL EVENT - Конкретное событие сигнала
// ============================================================================

/// Событие сигнала с контекстом
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEvent {
    /// Тип сигнала
    pub kind: SignalKind,
    /// Индекс бара
    pub bar_index: usize,
    /// Цена срабатывания
    pub price: f64,
    /// Сила сигнала (0.0 - 1.0)
    pub strength: f64,
    /// Дополнительный контекст
    pub context: Option<String>,
}

impl SignalEvent {
    pub fn new(kind: SignalKind, bar_index: usize, price: f64) -> Self {
        Self {
            kind,
            bar_index,
            price,
            strength: 1.0,
            context: None,
        }
    }

    pub fn with_strength(mut self, strength: f64) -> Self {
        self.strength = strength.clamp(0.0, 1.0);
        self
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_direction() {
        assert_eq!(SignalKind::CrossoverUp.direction(), 1);
        assert_eq!(SignalKind::CrossoverDown.direction(), -1);
        assert_eq!(SignalKind::ChannelSqueeze.direction(), 0);
    }

    #[test]
    fn test_signal_category() {
        assert_eq!(SignalKind::GoldenCross.category(), SignalCategory::Trend);
        assert_eq!(SignalKind::OscillatorOverbought.category(), SignalCategory::Momentum);
        assert_eq!(SignalKind::VolumeSpike.category(), SignalCategory::Volume);
    }

    #[test]
    fn test_signal_actionable() {
        assert!(SignalKind::CrossoverUp.is_actionable());
        assert!(SignalKind::GoldenCross.is_actionable());
        assert!(!SignalKind::PatternForming.is_actionable());
        assert!(!SignalKind::WeakSignal.is_actionable());
    }
}
