#![cfg(windows)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use yohaku_platform::foreground::ForegroundMonitor;
use yohaku_platform::media::MediaMonitor;
use yohaku_platform::system::SystemEvents;

fn assert_finishes(action: impl FnOnce() + Send + 'static) {
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        action();
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("monitor startup or shutdown did not finish within five seconds");
}

#[test]
fn foreground_monitor_can_be_dropped_immediately() {
    assert_finishes(|| {
        let (tx, _rx) = mpsc::channel();
        let sample = Arc::new(Mutex::new(None));
        drop(ForegroundMonitor::spawn(tx, sample));
    });
}

#[test]
fn system_events_can_be_dropped_immediately() {
    assert_finishes(|| {
        let (tx, _rx) = mpsc::channel();
        drop(SystemEvents::spawn(tx));
    });
}

#[test]
fn media_monitor_startup_and_stop_release_notifications() {
    assert_finishes(|| {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut monitor) = MediaMonitor::spawn(tx) {
            monitor.stop();
            monitor.stop();
        }
        while rx.try_recv().is_ok() {}
        assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    });
}
