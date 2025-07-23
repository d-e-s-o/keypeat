// Copyright (C) 2025 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Functionality for working with key repetitions.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::BitOrAssign;
use std::time::Duration;
use std::time::Instant;


/// Find the lesser of two `Option<Instant>` values.
///
/// Compared to using the default `Ord` impl of `Option`, `None` values
/// are actually strictly "greater" than any `Some`.
fn min_instant(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
  match (a, b) {
    (None, None) => None,
    (Some(_instant), None) => a,
    (None, Some(_instant)) => b,
    (Some(instant1), Some(instant2)) => Some(instant1.min(instant2)),
  }
}


/// The state a single key can be in.
#[derive(Clone, Copy, Debug)]
enum KeyState {
  Pressed {
    pressed_at: Instant,
    fire_count: usize,
  },
  Repeated {
    pressed_at: Instant,
    next_repeat: Instant,
    fire_count: usize,
  },
  ReleasePending {
    pressed_at: Instant,
    fire_count: usize,
  },
}

impl KeyState {
  fn pressed(pressed_at: Instant) -> Self {
    Self::Pressed {
      pressed_at,
      fire_count: 0,
    }
  }

  fn on_press(&mut self, now: Instant) {
    match self {
      Self::Pressed { .. } | Self::Repeated { .. } => {
        // If the key is already pressed we just got an AutoRepeat
        // event. We manage repetitions ourselves, so we skip any
        // handling.
      },
      Self::ReleasePending { fire_count, .. } => {
        // The key had been released, but some events were still
        // undelivered. Mark it as pressed again, and carry over said
        // events.
        *self = Self::Pressed {
          pressed_at: now,
          fire_count: *fire_count,
        }
      },
    }
  }

  fn on_release(&mut self, now: Instant, timeout: Duration, interval: Duration) {
    match self {
      Self::Pressed {
        pressed_at,
        fire_count,
      } => {
        let next_repeat = *pressed_at + timeout;
        if now >= next_repeat {
          // We hit the auto-repeat "threshold".
          *self = Self::Repeated {
            pressed_at: *pressed_at,
            next_repeat,
            fire_count: *fire_count + 1,
          };
          let () = self.on_release(now, timeout, interval);
        } else {
          *self = Self::ReleasePending {
            pressed_at: *pressed_at,
            fire_count: *fire_count + 1,
          }
        }
      },
      Self::Repeated {
        pressed_at,
        next_repeat,
        fire_count,
      } => {
        let diff = now.saturating_duration_since(*next_repeat);
        // TODO: Use `Duration::div_duration_f64` once stable.
        *fire_count += (diff.as_secs_f64() / interval.as_secs_f64()).trunc() as usize;
        // If `now` is past the next auto repeat, take that into account
        // as well.
        if now > *next_repeat {
          *fire_count += 1;
        }

        *self = Self::ReleasePending {
          pressed_at: *pressed_at,
          fire_count: *fire_count,
        }
      },
      Self::ReleasePending { .. } => {
        debug_assert!(false, "released key was not pressed");
      },
    }
  }

  fn next_tick(&self) -> Option<Instant> {
    match self {
      Self::Pressed { pressed_at, .. } => Some(*pressed_at),
      Self::Repeated {
        pressed_at,
        next_repeat,
        fire_count,
      } => {
        if *fire_count > 0 {
          Some(*pressed_at)
        } else {
          Some(*next_repeat)
        }
      },
      Self::ReleasePending {
        pressed_at,
        fire_count,
      } => {
        if *fire_count > 0 {
          Some(*pressed_at)
        } else {
          None
        }
      },
    }
  }

  /// # Notes
  /// This method should only be called once the `Instant` returned by
  /// [`KeyState::next_tick`] has been reached.
  fn tick(&mut self, timeout: Duration, interval: Duration) {
    match self {
      Self::Pressed {
        pressed_at,
        fire_count,
      } => {
        if let Some(count) = fire_count.checked_sub(1) {
          *fire_count = count;
        } else {
          *self = KeyState::Repeated {
            pressed_at: *pressed_at,
            next_repeat: *pressed_at + timeout,
            fire_count: 0,
          };
        }
      },
      Self::Repeated {
        next_repeat,
        fire_count,
        ..
      } => {
        if let Some(count) = fire_count.checked_sub(1) {
          *fire_count = count;
        } else {
          *next_repeat += interval;
        }
      },
      Self::ReleasePending { fire_count, .. } => {
        *fire_count = fire_count.saturating_sub(1);
      },
    }
  }
}


/// An enum representing the two possible auto-key-repeat states
/// supported.
#[derive(Debug)]
pub enum KeyRepeat {
  /// Auto-key-repeat is enabled.
  Enabled,
  /// Auto-key-repeat is disabled.
  Disabled,
}


/// A type tracking key states and implementing key auto-repeats at a
/// given interval after an initial "timeout".
///
/// In general, interaction with this object follows the pattern of
/// feeding key presses and releases as delivered by "the system" via
/// the [`on_key_press`][Keys::on_key_press] and
/// [`on_key_release`][Keys::on_key_release] methods. After that you
/// would [`tick`][Keys::tick] the object, which will invoke a handler
/// function for all the key presses and repeats accumulated since the
/// last time it was invoked.
///
/// For a complete and runnable example illustrating usage please refer
/// to [`winit-phys-events.rs`][winit-phys-events].
///
/// [winit-phys-events]: https://github.com/d-e-s-o/keypeat/blob/main/examples/winit-phys-events.rs
#[derive(Debug)]
pub struct Keys<K> {
  /// The "timeout" after the initial key press after which the first
  /// repeat is issued.
  timeout: Duration,
  /// The interval for any subsequent repeats.
  interval: Duration,
  /// A map from keys that are currently pressed to internally used
  /// key repetition state.
  ///
  /// The state may be `None` temporarily, in which case it is about to
  /// be removed.
  pressed: HashMap<K, Option<KeyState>>,
}

impl<K> Keys<K>
where
  K: Copy + Eq + Hash,
{
  /// Create a new [`Keys`] object using `timeout` as the initial
  /// timeout after which pressed keys transition into auto-repeat mode
  /// at interval `interval`.
  pub fn new(timeout: Duration, interval: Duration) -> Self {
    Self {
      timeout,
      interval,
      pressed: HashMap::new(),
    }
  }

  fn on_key_event(&mut self, now: Instant, key: K, pressed: bool) {
    match pressed {
      false => match self.pressed.entry(key) {
        Entry::Vacant(_vacancy) => {
          // Note that a key could be released without being marked here
          // as pressed anymore, if auto repeat had been disabled. In
          // such a case it is fine to just ignore the release.
        },
        Entry::Occupied(mut occupancy) => {
          if let Some(ref mut state) = occupancy.get_mut() {
            let () = state.on_release(now, self.timeout, self.interval);
          } else {
            let _state = occupancy.remove();
          }
        },
      },
      true => match self.pressed.entry(key) {
        Entry::Vacant(vacancy) => {
          let _state = vacancy.insert(Some(KeyState::pressed(now)));
        },
        Entry::Occupied(mut occupancy) => {
          if let Some(ref mut state) = occupancy.get_mut() {
            let () = state.on_press(now);
          } else {
            let _state = occupancy.insert(Some(KeyState::pressed(now)));
          }
        },
      },
    }
  }

  /// This method is to be invoked on every key press received.
  pub fn on_key_press(&mut self, now: Instant, key: K) {
    self.on_key_event(now, key, true)
  }

  /// This method is to be invoked on every key release received.
  pub fn on_key_release(&mut self, now: Instant, key: K) {
    self.on_key_event(now, key, false)
  }

  /// Handle a "tick", i.e., evaluate currently pressed keys based on
  /// the provided time, invoking `handler` for each overdue repeat
  /// event.
  ///
  /// `handler` can change the key's [`KeyRepeat`] state (key repetition
  /// is enabled by default).
  ///
  /// Furthermore, `handler` may return any kind of state that can be
  /// bitwise ORed, allowing to communicate an abstract notion of
  /// "changes triggered" to callers. In addition, the instant at which
  /// the next "tick" is likely to occur (and, hence, this function
  /// should be invoked) is returned as well (if any).
  // TODO: It could be beneficial to coalesce nearby ticks into a single
  //       one, to reduce the number of event loop wake ups.
  pub fn tick<F, C>(&mut self, now: Instant, mut handler: F) -> (C, Option<Instant>)
  where
    F: FnMut(&K, &mut KeyRepeat) -> C,
    C: Default + BitOrAssign,
  {
    let mut change = C::default();
    let mut next_tick = None;
    let mut remove = None;

    'next_key: for (key, key_state_opt) in self.pressed.iter_mut() {
      if let Some(key_state) = key_state_opt {
        loop {
          if let Some(tick) = key_state.next_tick() {
            if tick > now {
              next_tick = min_instant(next_tick, Some(tick));
              continue 'next_key
            }

            let mut repeat = KeyRepeat::Enabled;
            change |= handler(key, &mut repeat);

            match repeat {
              KeyRepeat::Disabled => {
                *key_state_opt = None;
                remove = remove.or(Some(*key));
                continue 'next_key
              },
              KeyRepeat::Enabled => {
                let () = key_state.tick(self.timeout, self.interval);
              },
            }
          } else {
            // If there is no next tick then the key had been released
            // earlier. Make sure to remove the state after we are done.
            *key_state_opt = None;
            remove = remove.or(Some(*key));
            continue 'next_key
          }
        }
      }
    }

    if let Some(key) = remove {
      // We only ever remove one key at a time to not have to allocate.
      // It won't take many invocations of this function to clear all
      // keys for which the "user" wants to disable auto-repeat, though.
      let _state = self.pressed.remove(&key);
      debug_assert!(_state.is_some());
    }

    (change, next_tick)
  }

  /// Clear all pressed keys, i.e., marking them all as released.
  #[inline]
  pub fn clear(&mut self) {
    self.pressed.clear()
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::cell::Cell;
  use std::ops::BitOr;

  type Key = char;

  /// A `Duration` of one second.
  const SECOND: Duration = Duration::from_secs(1);
  const TIMEOUT: Duration = Duration::from_secs(5);
  const INTERVAL: Duration = Duration::from_secs(1);


  #[derive(Clone, Copy, Debug, Default, PartialEq)]
  enum Change {
    #[default]
    Unchanged,
    Changed,
  }

  impl BitOr<Change> for Change {
    type Output = Change;

    fn bitor(self, rhs: Change) -> Self::Output {
      match (self, rhs) {
        (Self::Changed, _) | (_, Self::Changed) => Self::Changed,
        (Self::Unchanged, Self::Unchanged) => Self::Unchanged,
      }
    }
  }

  impl BitOrAssign<Change> for Change {
    fn bitor_assign(&mut self, rhs: Change) {
      *self = *self | rhs;
    }
  }


  /// Check that we correctly handle press-release sequences without an
  /// intermediate tick.
  #[test]
  fn press_release_without_tick() {
    let l_pressed = Cell::new(0);

    let mut handler = |key: &Key, _repeat: &mut KeyRepeat| match key {
      'l' => {
        l_pressed.set(l_pressed.get() + 1);
        Change::Changed
      },
      _ => Change::Unchanged,
    };

    let now = Instant::now();
    let mut keys = Keys::<Key>::new(TIMEOUT, INTERVAL);

    let () = keys.on_key_press(now, 'l');
    let () = keys.on_key_release(now + 1 * SECOND, 'l');
    let (change, tick) = keys.tick(now + 1 * SECOND, &mut handler);
    assert_eq!(l_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, None);

    let (change, tick) = keys.tick(now + 2 * SECOND, &mut handler);
    assert_eq!(l_pressed.get(), 1);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, None);
  }


  /// Check that we handle a press after a release without a tick as
  /// expected.
  #[test]
  fn press_after_release_pending() {
    let h_pressed = Cell::new(0);

    let mut handler = |key: &Key, _repeat: &mut KeyRepeat| match key {
      'h' => {
        h_pressed.set(h_pressed.get() + 1);
        Change::Changed
      },
      _ => Change::Unchanged,
    };

    let now = Instant::now();
    let mut keys = Keys::<Key>::new(TIMEOUT, INTERVAL);

    let () = keys.on_key_press(now, 'h');
    let () = keys.on_key_release(now + 1 * SECOND, 'h');
    let () = keys.on_key_press(now + 2 * SECOND, 'h');

    let (change, tick) = keys.tick(now + 2 * SECOND, &mut handler);
    assert_eq!(h_pressed.get(), 2);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 7 * SECOND));

    let (change, tick) = keys.tick(now + 3 * SECOND, &mut handler);
    assert_eq!(h_pressed.get(), 2);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, Some(now + 7 * SECOND));
  }


  /// Test that our `KeyState` logic works correctly when a key is
  /// released after auto-repeat already kicked in.
  #[test]
  fn release_pending_after_repeat() {
    let h_pressed = Cell::new(0);

    let mut handler = |key: &Key, _repeat: &mut KeyRepeat| match key {
      'h' => {
        h_pressed.set(h_pressed.get() + 1);
        Change::Changed
      },
      _ => Change::Unchanged,
    };

    let now = Instant::now();
    let mut keys = Keys::<Key>::new(TIMEOUT, INTERVAL);

    let () = keys.on_key_press(now, 'h');
    // Auto-repeat should kick in at `now + 5`. The one at `now + 7`
    // should not trigger, though, because of release.
    let () = keys.on_key_release(now + 7 * SECOND, 'h');

    let (change, tick) = keys.tick(now + 8 * SECOND, &mut handler);
    assert_eq!(h_pressed.get(), 4);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, None);
  }


  /// Check that keys are being reported as pressed as expected.
  #[test]
  fn key_pressing() {
    let enter_pressed = Cell::new(0);
    let space_pressed = Cell::new(0);
    let f_pressed = Cell::new(0);

    let mut handler = |key: &Key, repeat: &mut KeyRepeat| match key {
      '\n' => {
        enter_pressed.set(enter_pressed.get() + 1);
        Change::Changed
      },
      ' ' => {
        space_pressed.set(space_pressed.get() + 1);
        Change::Changed
      },
      'f' => {
        f_pressed.set(f_pressed.get() + 1);
        *repeat = KeyRepeat::Disabled;
        Change::Changed
      },
      _ => Change::Unchanged,
    };

    let mut keys = Keys::<Key>::new(TIMEOUT, INTERVAL);

    let now = Instant::now();
    let (change, tick) = keys.tick(now, &mut handler);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, None);

    let () = keys.on_key_press(now, '\n');
    let (change, tick) = keys.tick(now, &mut handler);
    assert_eq!(enter_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 5 * SECOND));

    // Another tick at the same timestamp shouldn't change anything.
    let (change, tick) = keys.tick(now, &mut handler);
    assert_eq!(enter_pressed.get(), 1);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, Some(now + 5 * SECOND));

    // Additional press events for the same key should just be ignored.
    let () = keys.on_key_press(now, '\n');

    // Or even half a second into the future.
    let (change, tick) = keys.tick(now + Duration::from_millis(500), &mut handler);
    assert_eq!(enter_pressed.get(), 1);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, Some(now + 5 * SECOND));

    // At t+5s we hit the auto-repeat timeout.
    let (change, tick) = keys.tick(now + 5 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 2);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 6 * SECOND));

    // Press F3 as well. That should be a one-time thing only, as the
    // handler disabled auto-repeat.
    let () = keys.on_key_press(now + 5 * SECOND, 'f');
    assert_eq!(f_pressed.get(), 0);

    // We skipped a couple of ticks and at t+8s we should see three
    // additional repeats.
    let (change, tick) = keys.tick(now + 8 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 5);
    assert_eq!(f_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 9 * SECOND));

    assert_eq!(space_pressed.get(), 0);
    // At t+9s we also press Space.
    let () = keys.on_key_press(now + 9 * SECOND, ' ');

    let (change, tick) = keys.tick(now + 10 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 7);
    assert_eq!(space_pressed.get(), 1);
    assert_eq!(f_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 11 * SECOND));

    // At t+15s we should see another 5 repeats for Enter as well as two
    // for Space.
    let (change, tick) = keys.tick(now + 15 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 12);
    assert_eq!(space_pressed.get(), 3);
    assert_eq!(f_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 16 * SECOND));

    // Space is released just "before" it's next tick, so we shouldn't
    // see a press fire.
    let () = keys.on_key_release(now + 16 * SECOND, ' ');

    let (change, tick) = keys.tick(now + 16 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 13);
    assert_eq!(space_pressed.get(), 3);
    assert_eq!(f_pressed.get(), 1);
    assert_eq!(change, Change::Changed);
    assert_eq!(tick, Some(now + 17 * SECOND));

    let () = keys.on_key_release(now + 17 * SECOND, '\n');

    let (change, tick) = keys.tick(now + 17 * SECOND, &mut handler);
    assert_eq!(enter_pressed.get(), 13);
    assert_eq!(space_pressed.get(), 3);
    assert_eq!(f_pressed.get(), 1);
    assert_eq!(change, Change::Unchanged);
    assert_eq!(tick, None);
  }
}
