//! Stand-in for the Skate guest: attaches to the shared region, beats, prints what the WoW
//! host publishes, and can ask for a teleport. Lets the host plugin be tested without Skate.
use skatecraft_protocol::*;
use std::{thread::sleep, time::Duration};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn main() {
    let shared = Shared::open().expect("open shared region");
    let pid = unsafe { GetCurrentProcessId() };
    println!("fake-guest attached, pid {pid}; waiting for WoW (Ctrl+C to stop)");
    let mut last_frame = u32::MAX;
    loop {
        shared.set_guest_beat(pid, unsafe { GetTickCount64() });
        let h = shared.header();
        let w = shared.read_wow();
        if w.frame != last_frame {
            last_frame = w.frame;
            println!(
                "wow pid {} beat {} | flags {:#x} map {} pos ({:.2}, {:.2}, {:.2}) facing {:.2}",
                h.wow_pid, h.wow_heartbeat_ms, w.flags, w.map_id, w.x, w.y, w.z, w.facing
            );
        }
        sleep(Duration::from_millis(50));
    }
}
