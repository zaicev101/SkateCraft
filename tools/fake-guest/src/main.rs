//! Stand-in for the Skate guest: attaches to the shared region, beats, prints what the WoW
//! host publishes, and with `circle` drives the player around a circle (the puppet test).
//!   fake-guest            just watch
//!   fake-guest circle     walk a 6-yard-radius circle around where the player stands
use skatecraft_protocol::*;
use std::{thread::sleep, time::Duration};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn main() {
    let circle = std::env::args().nth(1).as_deref() == Some("circle");
    let shared = Shared::open().expect("open shared region");
    let pid = unsafe { GetCurrentProcessId() };
    println!("fake-guest attached, pid {pid}{}", if circle { ", circle mode" } else { "" });
    let mut last_frame = u32::MAX;
    let mut center: Option<WowState> = None;
    let t0 = unsafe { GetTickCount64() };
    loop {
        let now = unsafe { GetTickCount64() };
        shared.set_guest_beat(pid, now);
        let w = shared.read_wow();
        if circle && w.flags & wow_flags::IN_WORLD != 0 {
            let c = *center.get_or_insert(w);
            let t = (now - t0) as f64 / 1000.0;
            let r = 6.0;
            let speed = 7.0; // yards per second
            let a = t * speed / r;
            shared.write_guest(GuestState {
                flags: guest_flags::ALIVE | guest_flags::DRIVING,
                x: c.x + r * a.cos() - r,
                y: c.y + r * a.sin(),
                z: c.z,
                // tangent direction in WoW terms (x north, y west, counter-clockwise facing)
                facing: ((a + std::f64::consts::FRAC_PI_2).sin().atan2((a + std::f64::consts::FRAC_PI_2).cos())) as f32,
                speed: speed as f32,
                ..Default::default()
            });
        } else if !circle {
            center = None;
        }
        if w.frame != last_frame && !circle {
            last_frame = w.frame;
            println!("wow pid {} | flags {:#x} map {} pos ({:.2}, {:.2}, {:.2}) facing {:.2}", shared.header().wow_pid, w.flags, w.map_id, w.x, w.y, w.z, w.facing);
        }
        sleep(Duration::from_millis(if circle { 8 } else { 50 }));
    }
}
