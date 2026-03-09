# Test Checklist for Indicators

## Bugs Found
- IchimokuCloud: senkou_b_filled was checking kijun_buffer length (max 26) against senkou_b_period (52). Fixed by using dedicated senkou_b_high/low_buffer.

---

## CHANNELS (43 files) ✅ DONE

---

## VOLATILITY (42 files) ✅ DONE

## STATISTICS (26 files) ✅ DONE

## LEVELS (19 files) ✅ DONE

## POSITION (18 files) ✅ DONE

## VOLUME (17 files) ✅ DONE

## TREND (15 files) ✅ DONE

## DIVERGENCE (14 files)
- [ ] cci_divergence
- [ ] classic_divergence
- [ ] divergence
- [ ] divergence_strength
- [ ] hidden_divergence
- [ ] macd_divergence
- [ ] macd_histogram_divergence
- [ ] momentum_divergence
- [ ] multi_divergence
- [ ] obv_divergence
- [ ] rsi_divergence
- [ ] stochastic_divergence
- [ ] volume_divergence
- [ ] williams_divergence

## ACCUMULATION (12 files)
- [ ] accumulation_distribution
- [ ] accumulative_swing_index
- [ ] chaikin_money_flow
- [ ] chaikin_oscillator
- [ ] demand_index
- [ ] ease_of_movement
- [ ] force_index
- [ ] intraday_intensity
- [ ] intraday_intensity_percent
- [ ] intraday_intensity_ratio
- [ ] tmf
- [ ] williams_ad

## ENTROPY (12 files)
- [ ] approximate_entropy
- [ ] conditional_entropy
- [ ] cross_mutual_information_lags
- [ ] fisher_information
- [ ] information_gain
- [ ] js_divergence
- [ ] kl_divergence
- [ ] mutual_information
- [ ] permutation_entropy
- [ ] sample_entropy
- [ ] shannon_entropy
- [ ] transfer_entropy

## KALMAN (11 files)
- [ ] alpha_beta_gamma_filter
- [ ] basic_kalman_filter
- [ ] extended_kalman_filter
- [ ] kalman_regime_composite
- [ ] kalman_regime_score
- [ ] kalman_slope_zscore
- [ ] kalman_trend_regime
- [ ] kalman_trend_slope
- [ ] particle_filter
- [ ] rts_smoother
- [ ] unscented_kalman_filter

## TREND_STOP (10 files)
- [ ] atr_trailing_stop
- [ ] chandelier_stop
- [ ] chande_kroll_stop
- [ ] donchian_breakout
- [ ] donchian_stop
- [ ] keltner_stop
- [ ] psar_stop
- [ ] supertrend_stop
- [ ] swing_stop
- [ ] volatility_stop

## CHAOS (8 files)
- [ ] chaos_oscillator
- [ ] dfa
- [ ] dfa_percentile
- [ ] fractal_dimension
- [ ] hurst_exponent
- [ ] hurst_percentile
- [ ] williams_fractals
- [ ] williams_indicators

## ZIGZAG (6 files)
- [ ] factory
- [ ] zigzag_atr
- [ ] zigzag_candle
- [ ] zigzag_classic
- [ ] zigzag_lookahead
- [ ] zigzag_time

## CLUSTERS (6 files)
- [ ] order_book_slope
- [ ] queue_imbalance
- [ ] order_flow_imbalance
- [ ] volume_weighted_price_levels
- [ ] market_microstructure
- [ ] tick_volume_analyzer

## ADAPTIVE (5 files)
- [ ] adaptive_moving_average
- [ ] frama
- [ ] kaufman_adaptive_ma
- [ ] vidya
- [ ] mesa_adaptive_ma

## CANDLES (5 files)
- [ ] candle_anatomy
- [ ] heikin_ashi
- [ ] pattern_recognition
- [ ] sfp_detector
- [ ] wick_spike

## RATIO (4 files)
- [ ] efficiency_ratio
- [ ] efficiency_ratio_ring
- [ ] range_to_atr
- [ ] spread_analyzer

## REGRESSION (4 files)
- [ ] arima
- [ ] garch
- [ ] polynomial
- [ ] var

## BOOK (1 file)
- [ ] imbalance

---
TOTAL: ~230 files to add tests
PROGRESS: signal_processing (53) ✅ complete, channels (43) ✅ complete, volatility (42) ✅ complete, statistics (26) ✅ complete, levels (19) ✅ complete, position (18) ✅ complete, volume (17) ✅ complete, trend (15) ✅ complete
