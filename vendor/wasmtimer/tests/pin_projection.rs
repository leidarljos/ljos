use std::{
    cell::Cell,
    future::{Future, Ready},
    marker::PhantomPinned,
    pin::Pin,
    rc::Rc,
    sync::Once,
    task::{Context, Poll},
    time::Duration,
};

use futures::task::noop_waker_ref;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use wasmtimer::tokio::{advance, pause, timeout, Interval, Timeout};

#[cfg(browser)]
wasm_bindgen_test_configure!(run_in_browser);

static INIT: Once = Once::new();

fn initialize() {
    INIT.call_once(pause);
}

struct PinnedFuture {
    polls: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
    ready_after: usize,
    value: u8,
    _pin: PhantomPinned,
}

impl Future for PinnedFuture {
    type Output = u8;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_ref().get_ref();
        let count = this.polls.get() + 1;
        this.polls.set(count);
        if count >= this.ready_after {
            Poll::Ready(this.value)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for PinnedFuture {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn future(ready_after: usize) -> (PinnedFuture, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let polls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    (
        PinnedFuture {
            polls: Rc::clone(&polls),
            drops: Rc::clone(&drops),
            ready_after,
            value: 73,
            _pin: PhantomPinned,
        },
        polls,
        drops,
    )
}

#[wasm_bindgen_test]
fn a_non_unpin_future_keeps_its_value_and_single_drop() {
    initialize();
    let (inner, polls, drops) = future(2);
    let mut wrapped = Box::pin(timeout(Duration::from_secs(60), inner));
    let mut cx = Context::from_waker(noop_waker_ref());
    assert!(wrapped.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        wrapped.as_mut().poll(&mut cx),
        Poll::Ready(Ok(73))
    ));
    assert_eq!(polls.get(), 2);
    assert_eq!(drops.get(), 0);
    drop(wrapped);
    assert_eq!(drops.get(), 1);
}

#[wasm_bindgen_test]
async fn a_pending_pinned_future_reaches_the_delay_and_drops_once() {
    initialize();
    let (inner, polls, drops) = future(usize::MAX);
    let mut wrapped = Box::pin(timeout(Duration::from_millis(100), inner));
    let mut cx = Context::from_waker(noop_waker_ref());
    assert!(wrapped.as_mut().poll(&mut cx).is_pending());
    advance(Duration::from_millis(110)).await;
    assert!(matches!(
        wrapped.as_mut().poll(&mut cx),
        Poll::Ready(Err(_))
    ));
    assert_eq!(polls.get(), 2);
    assert_eq!(drops.get(), 0);
    drop(wrapped);
    assert_eq!(drops.get(), 1);
}

#[wasm_bindgen_test]
fn owned_accessors_and_existing_unpin_types_keep_their_contract() {
    initialize();
    fn assert_unpin<T: Unpin>() {}
    assert_unpin::<Interval>();
    assert_unpin::<Timeout<Ready<u8>>>();

    let (inner, polls, drops) = future(1);
    let mut wrapped = timeout(Duration::from_secs(60), inner);
    assert_eq!(wrapped.get_ref().value, 73);
    wrapped.get_mut().value = 19;
    assert_eq!(wrapped.get_ref().value, 19);
    let returned = wrapped.into_inner();
    assert_eq!(returned.value, 19);
    assert_eq!(polls.get(), 0);
    assert_eq!(drops.get(), 0);
    drop(returned);
    assert_eq!(drops.get(), 1);
}
