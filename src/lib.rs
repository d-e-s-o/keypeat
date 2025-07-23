// Copyright (C) 2025 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A library providing auto-repeat functionality for an arbitrary
//! number of keys based on key press and release events.
//!
//! The library aims to be flexible enough to cater to all sorts of
//! event loop setups and event handling paradigms.
//!
//! # Background
//! Most windowing systems come with built-in key auto-repeat support.
//! However, when dealing with raw physical device events, that may not
//! be the case.
//! This library provides the means for building your own auto-repeat
//! support, using timing independent of any system settings. This can
//! be relevant for games or simulations, for example, where users may
//! want to be able to influence these timings without having to make
//! system-wide changes.

mod keys;

pub use keys::KeyRepeat;
pub use keys::Keys;
