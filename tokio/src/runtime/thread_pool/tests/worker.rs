use crate::runtime::blocking;
use crate::runtime::tests::track_drop::track_drop;
use crate::runtime::thread_pool;

use tokio_test::assert_ok;

use std::sync::Arc;

macro_rules! pool {
    (2) => {{
        let (pool, mut w, mock_park) = pool!(!2);
        (pool, w.remove(0), w.remove(0), mock_park)
    }};
    (! $n:expr) => {{
        let mut mock_park = crate::runtime::tests::mock_park::MockPark::new();
        let blocking = blocking::Pool::new("test".into(), None);
        let (pool, workers) = thread_pool::worker::create_set(
            $n,
            |index| Box::new(mock_park.mk_park(index)),
            Arc::new(Box::new(|_| {
                unreachable!("attempted to move worker during non-blocking test")
            })),
            blocking,
        );
        (pool, workers, mock_park)
    }};
}

macro_rules! enter {
    ($w:expr, $expr:expr) => {{
        $w.enter(move || $expr);
    }};
}

#[test]
fn execute_single_task() {
    use std::sync::mpsc;

    let (p, mut w0, _w1, ..) = pool!(2);
    let (tx, rx) = mpsc::channel();

    enter!(w0, p.spawn_background(async move { tx.send(1).unwrap() }));

    w0.tick();

    assert_ok!(rx.try_recv());
}

#[test]
fn task_migrates() {
    use crate::sync::oneshot;
    use std::sync::mpsc;

    let (p, mut w0, mut w1, ..) = pool!(2);
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = mpsc::channel();

    let (task, did_drop) = track_drop(async move {
        let msg = rx1.await.unwrap();
        tx2.send(msg).unwrap();
    });

    enter!(w0, p.spawn_background(task));

    w0.tick();
    w1.enter(|| tx1.send("hello").unwrap());

    w1.tick();
    assert_ok!(rx2.try_recv());

    // Future drops immediately even though the underlying task is not freed
    assert!(did_drop.did_drop_future());
    assert!(did_drop.did_drop_output());

    // Tick the spawning worker in order to free memory
    w0.tick();
}
