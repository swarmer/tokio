#![cfg(unix)]
#![warn(rust_2018_idioms)]

mod support {
    pub mod signal;
}
use support::signal::send_signal;

use tokio::prelude::*;
use tokio::runtime::Runtime;
use tokio::signal::unix::{signal, SignalKind};

#[test]
fn dropping_loops_does_not_cause_starvation() {
    let kind = SignalKind::user_defined1();

    let mut first_rt = rt();
    let mut first_signal =
        first_rt.block_on(async { signal(kind).expect("failed to register first signal") });

    let mut second_rt = rt();
    let mut second_signal =
        second_rt.block_on(async { signal(kind).expect("failed to register second signal") });

    send_signal(libc::SIGUSR1);

    first_rt
        .block_on(first_signal.next())
        .expect("failed to await first signal");

    drop(first_rt);
    drop(first_signal);

    send_signal(libc::SIGUSR1);

    second_rt.block_on(second_signal.next());
}

fn rt() -> Runtime {
    tokio::runtime::Builder::new()
        .current_thread()
        .build()
        .unwrap()
}
