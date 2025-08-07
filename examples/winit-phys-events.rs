// Copyright (C) 2025 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! An example illustrating usage of `keypeat` in conjunction with
//! `winit` and raw physical key press handling to implement key
//! auto-repeat.

use std::env::args_os;
use std::mem::MaybeUninit;
use std::process::ExitCode;
use std::time::Duration;
use std::time::Instant;

use keypeat::KeyRepeat;
use keypeat::Keys;

use winit::application::ApplicationHandler;
use winit::event::DeviceEvent;
use winit::event::DeviceId;
use winit::event::ElementState;
use winit::event::RawKeyEvent;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::ControlFlow;
use winit::event_loop::EventLoop;
use winit::keyboard::KeyCode as Key;
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;


const FAST_PRESET: (Duration, Duration) = (Duration::from_millis(60), Duration::from_millis(25));
const MED_PRESET: (Duration, Duration) = (Duration::from_millis(500), Duration::from_millis(200));
const SLOW_PRESET: (Duration, Duration) =
  (Duration::from_millis(2000), Duration::from_millis(1000));


struct App {
  keys: Keys<Key>,
}

impl App {
  fn new(keys: Keys<Key>) -> Self {
    Self { keys }
  }
}

impl ApplicationHandler for App {
  fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

  fn window_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    _window_id: WindowId,
    _event: WindowEvent,
  ) {
  }

  /// A handler for "device" events.
  fn device_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    _device_id: DeviceId,
    event: DeviceEvent,
  ) {
    // Handle all raw key events and ignore everything else. These
    // events do not include any auto-repeats, as this is typically a
    // software construct.
    if let DeviceEvent::Key(RawKeyEvent {
      physical_key: PhysicalKey::Code(key),
      state,
    }) = event
    {
      println!(
        "physical key {}: {key:?}",
        if matches!(state, ElementState::Pressed) {
          "press"
        } else {
          "release"
        }
      );

      let now = Instant::now();
      match state {
        ElementState::Pressed => self.keys.on_key_press(now, key),
        ElementState::Released => self.keys.on_key_release(now, key),
      }
    }
  }

  /// A callback invoked when `winit` is about to enter a "wait" state,
  /// waiting for either the next external event or a configurable point
  /// in the future at which to wake up.
  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    let handle_key = |key: &Key, repeat: &mut KeyRepeat| {
      match key {
        Key::Escape => {
          // Disable auto-repeat for this key. This is mostly done for
          // illustration purposes, as we are about to quit anyway.
          *repeat = KeyRepeat::Disabled;
          // Indicate to the caller our intention to quit the program.
          true
        },
        _ => {
          // All other keys we just print.
          println!("virtual key press: {key:?}");
          false
        },
      }
    };

    let now = Instant::now();
    // Check for any key-presses just encountered as well as
    // auto-repeats accumulated and invoke `handle_key` for each.
    // Returned is a tuple of a caller controlled value (in this case a
    // boolean flag indicating whether to exit the program) as well as
    // an `Option` potentially containing the next point in time when an
    // auto-repeat will trigger based on currently pressed (and not yet
    // released) keys.
    let (quit, wait_until) = self.keys.tick(now, handle_key);

    if quit {
      let () = event_loop.exit();
      return
    }

    let control_flow = if let Some(wait_until) = wait_until {
      ControlFlow::WaitUntil(wait_until)
    } else {
      ControlFlow::Wait
    };
    let () = event_loop.set_control_flow(control_flow);
  }
}


/// Enable or disable automatic echoing of input characters to the
/// terminal.
fn enable_echo(enable: bool) {
  let mut raw = MaybeUninit::<libc::termios>::uninit();
  let rc = unsafe { libc::tcgetattr(libc::STDIN_FILENO, raw.as_mut_ptr()) };
  assert_eq!(rc, 0, "tcgetattr() failed");

  let mut raw = unsafe { raw.assume_init() };
  if enable {
    raw.c_lflag |= libc::ECHO;
  } else {
    raw.c_lflag &= !(libc::ECHO);
  }
  let rc = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) };
  assert_eq!(rc, 0, "tcsetattr() failed");
}


fn main() -> ExitCode {
  let (timeout, interval) = match args_os().len() {
    0 | 1 => MED_PRESET,
    2 if args_os().any(|arg| &arg == "--slow") => SLOW_PRESET,
    2 if args_os().any(|arg| &arg == "--fast") => FAST_PRESET,
    _ => {
      eprintln!("encountered unsupported number of program arguments");
      return ExitCode::FAILURE
    },
  };

  let keys = Keys::new(timeout, interval);
  let event_loop = EventLoop::new().unwrap();
  let mut app = App::new(keys);

  println!("Custom key auto-repeat is in effect");
  println!("Press and hold one or more keys to see key repeat behavior.");
  println!("Restart with --slow or --fast to active different timing settings");
  println!("Press Esc to exit");

  let () = enable_echo(false);
  let () = event_loop.run_app(&mut app).unwrap();
  let () = enable_echo(true);
  ExitCode::SUCCESS
}
