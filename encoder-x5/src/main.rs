//! Horizon X5 VPU encoder worker binary.
//!
//! This is a standalone process that will be spawned by the controller
//! to handle hardware-accelerated encoding on Horizon Robotics X5 SoCs.
//! Communication with the main encoder orchestrator happens over
//! iceoryx2 shared-memory IPC (future sprint).

mod backend;

fn main() {
    eprintln!("rollio-encoder-x5: stub — IPC worker not yet wired");
    std::process::exit(1);
}
