use pcap::{Capture, Device};
use std::thread;
mod tui_plotter;
use tui_plotter::NetworkPlotter;

fn main() {
    // Create the network plotter
    let plotter = NetworkPlotter::new();
    let plotter_clone = plotter.get_state();

    // Start the TUI in a separate thread
    let tui_thread = thread::spawn(move || {
        plotter.start_ui().unwrap();
    });

    // Set up pcap
    let device = Device::lookup().unwrap().unwrap();
    let mut cap = Capture::from_device(device)
        .unwrap()
        .promisc(true)
        .open()
        .unwrap();

    while let Ok(packet) = cap.next_packet() {
        let packet_size = packet.data.len();

        // Update the plotter with the new packet data
        if let Ok(mut state) = plotter_clone.lock() {
            state.update_stats(packet_size);

            // Check if the TUI is still running
            if !state.running {
                break;
            }
        }
    }

    if let Err(e) = tui_thread.join() {
        eprintln!("TUI thread panicked: {:?}", e);
    }
}

