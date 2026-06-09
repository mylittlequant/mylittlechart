//! Demo Data Generation Module
//!
//! This module provides demo data generation for testing and development.
//! It can be replaced with real API integrations from the nemo library.
//!
//! # Contents
//!
//! - `symbols` - Demo symbol definitions (10 symbols across different categories)
//! - `data_generator` - OHLC data generation with consistent seeding per symbol
//! - `indicator_calc` - Demo indicator calculations (SMA, EMA, RSI, etc.)
//!
//! # Future Integration
//!
//! This entire module is designed to be swappable. When connecting to real APIs:
//! 1. Create a trait for data providers
//! 2. Implement demo provider using this module
//! 3. Implement real provider using nemo library
//! 4. Switch providers via configuration

mod symbols;
mod data_generator;
mod indicator_calc;

pub use symbols::*;
pub use data_generator::*;
pub use indicator_calc::*;
