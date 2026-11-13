//! Sources report what agents are doing. Each runs on its own thread and
//! only ever sends `BusEvent`s; none of them touches the keyboard.

pub mod opencode;
pub mod socket;
